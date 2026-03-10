use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

use rewind_cn_core::application::commands::{CreateEpic, CreateTask};
use rewind_cn_core::application::planning::{passthrough_plan, Plan, PlanGenerator};
use rewind_cn_core::domain::events::{AcceptanceCriterion, RewindEvent};
use rewind_cn_core::domain::ids::TaskId;
use rewind_cn_core::infrastructure::engine::RewindEngine;
use rewind_cn_core::infrastructure::llm::create_anthropic_client;
use rewind_cn_core::infrastructure::planner::PlannerAgent;

use rig::completion::Prompt;
use rig::prelude::CompletionClient;

use crate::config::RewindConfig;

const DATA_DIR: &str = ".rewind/data";
const CONFIG_FILE: &str = ".rewind/rewind.toml";
const PLAN_READY_MARKER: &str = "[PLAN_READY]";

pub async fn execute(
    description: Option<String>,
    file: Option<PathBuf>,
    dry_run: bool,
) -> Result<(), String> {
    if !Path::new(".rewind").exists() {
        return Err("No rewind project found. Run `rewind init` first.".into());
    }

    // Load config to check for agent section
    let config = RewindConfig::load(Path::new(CONFIG_FILE)).ok();

    // Determine mode: interactive, single-shot LLM, or passthrough
    let is_interactive = description.is_none() && file.is_none() && atty::is(atty::Stream::Stdin);

    let plan = if is_interactive {
        if let Some(ref cfg) = config {
            if let Some(ref agent_config) = cfg.agent {
                interactive_plan(agent_config).await?
            } else {
                return Err(
                    "Interactive mode requires [agent] config in .rewind/rewind.toml.\n\
                     Add an [agent] section or provide input: rewind plan \"description\""
                        .into(),
                );
            }
        } else {
            return Err(
                "Interactive mode requires [agent] config in .rewind/rewind.toml.\n\
                 Add an [agent] section or provide input: rewind plan \"description\""
                    .into(),
            );
        }
    } else {
        let input = resolve_input(description, file)?;
        if let Some(ref cfg) = config {
            if let Some(ref agent_config) = cfg.agent {
                eprintln!("Using LLM planner ({})...", agent_config.planner.model);
                let client = create_anthropic_client(agent_config).map_err(|e| e.to_string())?;
                let planner = PlannerAgent::new(client, agent_config.clone());
                planner.decompose(&input).await.map_err(|e| e.to_string())?
            } else {
                passthrough_plan(&input)
            }
        } else {
            passthrough_plan(&input)
        }
    };

    print_plan(&plan);

    if dry_run {
        println!("[dry run] — no events persisted.");
        return Ok(());
    }

    persist_plan(&plan).await
}

/// Interactive planning mode — conversation loop with the planner LLM.
async fn interactive_plan(
    agent_config: &rewind_cn_core::infrastructure::llm::AgentConfig,
) -> Result<Plan, String> {
    let client = create_anthropic_client(agent_config).map_err(|e| e.to_string())?;

    let agent = client
        .agent(&agent_config.planner.model)
        .preamble(INTERACTIVE_PLANNER_PROMPT)
        .max_tokens(agent_config.planner.max_tokens as u64)
        .build();

    println!("Interactive planning mode. Describe what you want to build.");
    println!("The planner will ask clarifying questions before generating a plan.");
    println!("Type 'done' to generate the plan from what's been discussed.\n");

    let stdin = io::stdin();
    let mut history = Vec::new();

    // Get initial description
    print!("> ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut initial = String::new();
    stdin
        .lock()
        .read_line(&mut initial)
        .map_err(|e| e.to_string())?;
    let initial = initial.trim().to_string();

    if initial.is_empty() {
        return Err("No description provided.".into());
    }

    // Send initial description
    history.push(format!("User: {initial}"));
    let prompt = format!(
        "The user wants to build something. Here's their initial description:\n\n{initial}\n\n\
         Ask 2-3 clarifying questions to understand the scope, then generate the plan."
    );

    let mut last_response: String = agent
        .prompt(&prompt)
        .await
        .map_err(|e| format!("Planner error: {e}"))?;

    // Conversation loop
    loop {
        // Check if the response contains the plan marker
        if last_response.contains(PLAN_READY_MARKER) {
            // Extract and parse the plan JSON
            return extract_plan_from_response(&last_response);
        }

        // Show the planner's response
        println!("\nPlanner: {last_response}\n");
        history.push(format!("Planner: {last_response}"));

        // Get user input
        print!("> ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut user_input = String::new();
        stdin
            .lock()
            .read_line(&mut user_input)
            .map_err(|e| e.to_string())?;
        let user_input = user_input.trim().to_string();

        if user_input.eq_ignore_ascii_case("done") || user_input.eq_ignore_ascii_case("quit") {
            // Ask the planner to generate the plan from what we have
            let context = history.join("\n");
            let final_prompt = format!(
                "Based on our conversation:\n{context}\n\n\
                 Now generate the final plan. Output {PLAN_READY_MARKER} followed by the JSON plan."
            );

            last_response = agent
                .prompt(&final_prompt)
                .await
                .map_err(|e| format!("Planner error: {e}"))?;

            return extract_plan_from_response(&last_response);
        }

        history.push(format!("User: {user_input}"));

        // Continue conversation
        let context = history.join("\n");
        let follow_up = format!(
            "Conversation so far:\n{context}\n\n\
             Continue the conversation. If you have enough information, output {PLAN_READY_MARKER} \
             followed by the JSON plan. Otherwise, ask more questions."
        );

        last_response = agent
            .prompt(&follow_up)
            .await
            .map_err(|e| format!("Planner error: {e}"))?;
    }
}

/// Extract a Plan from the planner's response containing [PLAN_READY].
fn extract_plan_from_response(response: &str) -> Result<Plan, String> {
    let after_marker = response.split(PLAN_READY_MARKER).nth(1).unwrap_or(response);

    let trimmed = after_marker.trim();
    let json_str = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s: &str| s.strip_suffix("```"))
        .unwrap_or(trimmed);

    serde_json::from_str::<Plan>(json_str)
        .map_err(|e| format!("Failed to parse plan from planner output: {e}\n\nRaw:\n{response}"))
}

fn print_plan(plan: &Plan) {
    println!("Epic: {}", plan.epic_title);
    if !plan.quality_gates.is_empty() {
        println!(
            "Quality Gates: {}",
            plan.quality_gates
                .iter()
                .map(|g| g.command.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!();

    for (i, story) in plan.stories.iter().enumerate() {
        let type_tag = story
            .story_type
            .as_ref()
            .map(|t| format!(" [{t:?}]"))
            .unwrap_or_default();
        println!("  {}. {}{}", i + 1, story.title, type_tag);

        for criterion in &story.acceptance_criteria {
            println!("     - [ ] {criterion}");
        }

        if !story.depends_on.is_empty() {
            let deps: Vec<String> = story
                .depends_on
                .iter()
                .map(|d| format!("US-{}", d + 1))
                .collect();
            println!("     deps: {}", deps.join(", "));
        }
    }
    println!();
}

async fn persist_plan(plan: &Plan) -> Result<(), String> {
    let engine = RewindEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    let epic_events = engine
        .create_epic(CreateEpic {
            title: plan.epic_title.clone(),
            description: plan.epic_description.clone(),
            quality_gates: plan.quality_gates.clone(),
        })
        .await
        .map_err(|e| e.to_string())?;

    let epic_id = match &epic_events[0] {
        RewindEvent::EpicCreated { epic_id, .. } => epic_id.clone(),
        _ => return Err("Unexpected event type".into()),
    };

    let mut task_ids: Vec<TaskId> = Vec::new();

    for story in &plan.stories {
        let depends_on: Vec<TaskId> = story
            .depends_on
            .iter()
            .filter_map(|&idx| task_ids.get(idx).cloned())
            .collect();

        let criteria: Vec<AcceptanceCriterion> = story
            .acceptance_criteria
            .iter()
            .map(|desc| AcceptanceCriterion {
                description: desc.clone(),
                checked: false,
            })
            .collect();

        let events = engine
            .create_task(CreateTask {
                title: story.title.clone(),
                description: story.description.clone(),
                epic_id: Some(epic_id.clone()),
                acceptance_criteria: criteria,
                story_type: story.story_type.clone(),
                depends_on,
            })
            .await
            .map_err(|e| e.to_string())?;

        let task_id = match &events[0] {
            RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
            _ => return Err("Unexpected event type".into()),
        };
        task_ids.push(task_id);
    }

    println!(
        "Created 1 epic with {} stor{}.",
        plan.stories.len(),
        if plan.stories.len() == 1 { "y" } else { "ies" }
    );

    Ok(())
}

fn resolve_input(description: Option<String>, file: Option<PathBuf>) -> Result<String, String> {
    if let Some(desc) = description {
        return Ok(desc);
    }

    if let Some(path) = file {
        return std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read file {}: {e}", path.display()));
    }

    // Read from stdin (piped)
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("Failed to read stdin: {e}"))?;

    if input.trim().is_empty() {
        return Err(
            "No input provided. Usage: rewind plan \"description\" or rewind plan -f file.md"
                .into(),
        );
    }

    Ok(input)
}

const INTERACTIVE_PLANNER_PROMPT: &str = r#"You are an interactive software project planner. You're having a conversation with a developer to understand what they want to build, then decompose it into an epic with stories.

## Conversation Flow
1. The user describes what they want to build
2. Ask 2-3 clarifying questions about scope, dependencies, and quality gates
3. When you have enough info, output [PLAN_READY] followed by the JSON plan

## Plan JSON Format
When ready, output [PLAN_READY] then a JSON object:
{
  "epic_title": "...",
  "epic_description": "...",
  "quality_gates": [{"command": "cargo test", "tier": "Epic"}],
  "stories": [
    {
      "title": "US-001: ...",
      "description": "...",
      "story_type": "Backend",
      "acceptance_criteria": ["Verifiable criterion 1", "..."],
      "depends_on": []
    }
  ]
}

## Rules
- Each story must be completable in one agent session
- Acceptance criteria must be concretely verifiable
- Story types: Schema, Backend, UI, Integration, Infrastructure
- depends_on uses 0-based indices into the stories array
- 3-10 stories per epic
"#;
