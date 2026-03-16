//! Import tasks and epics from external formats (beads JSONL, generic JSON).

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::application::commands::{CreateEpic, CreateTask};
use crate::domain::events::{AcceptanceCriterion, QualityGate, RewindEvent};
use crate::domain::ids::TaskId;
use crate::infrastructure::engine::RewindEngine;

/// A single issue entry from the beads JSONL format (cn CLI).
#[derive(Debug, Deserialize)]
pub struct BeadIssue {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub issue_type: String,
    #[serde(default)]
    pub dependencies: Vec<BeadDependency>,
    #[serde(default)]
    pub external_ref: Option<String>,
    #[serde(default)]
    pub comments: Vec<BeadComment>,
}

#[derive(Debug, Deserialize)]
pub struct BeadDependency {
    pub issue_id: String,
    pub depends_on_id: String,
    #[serde(rename = "type", default)]
    pub dep_type: String,
}

#[derive(Debug, Deserialize)]
pub struct BeadComment {
    #[serde(default)]
    pub text: String,
}

/// Result of an import operation.
#[derive(Debug)]
pub struct ImportResult {
    pub epics_created: usize,
    pub tasks_created: usize,
    pub skipped: usize,
}

/// Parse a beads JSONL file into a list of issues.
pub fn parse_beads_jsonl(content: &str) -> Result<Vec<BeadIssue>, String> {
    let mut issues = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let issue: BeadIssue = serde_json::from_str(line)
            .map_err(|e| format!("Line {}: failed to parse: {e}", line_num + 1))?;
        issues.push(issue);
    }
    Ok(issues)
}

/// Extract acceptance criteria from a description by looking for `- [ ]` and `- [x]` patterns.
pub fn extract_criteria_from_description(description: &str) -> Vec<AcceptanceCriterion> {
    let mut criteria = Vec::new();
    for line in description.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            criteria.push(AcceptanceCriterion {
                description: rest.to_string(),
                checked: false,
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            criteria.push(AcceptanceCriterion {
                description: rest.to_string(),
                checked: true,
            });
        }
    }
    criteria
}

/// Extract quality gates from an epic description by looking for backtick-quoted commands
/// in lines that look like `- [ ] \`command\`` patterns.
pub fn extract_quality_gates_from_description(description: &str) -> Vec<QualityGate> {
    let mut gates = Vec::new();
    for line in description.lines() {
        let trimmed = line.trim();
        // Match patterns like: - [ ] `cargo test` passes
        if (trimmed.starts_with("- [ ] ")
            || trimmed.starts_with("- [x] ")
            || trimmed.starts_with("- [X] "))
            && trimmed.contains('`')
        {
            // Extract text between backticks
            if let Some(start) = trimmed.find('`') {
                if let Some(end) = trimmed[start + 1..].find('`') {
                    let command = &trimmed[start + 1..start + 1 + end];
                    if !command.is_empty() {
                        gates.push(QualityGate {
                            command: command.to_string(),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }
    gates
}

/// Import beads JSONL issues into the engine.
///
/// - `issue_type: "epic"` → CreateEpic
/// - `issue_type: "task"` → CreateTask (linked to parent epic via dependencies)
/// - Closed issues are skipped
pub async fn import_beads<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
    issues: &[BeadIssue],
    engine: &RewindEngine<B>,
    skip_closed: bool,
) -> Result<ImportResult, String> {
    let mut epics_created = 0;
    let mut tasks_created = 0;
    let mut skipped = 0;

    // First pass: create epics and build ID mapping
    // bead_id -> (rewind_epic_id | rewind_task_id)
    let mut id_map: HashMap<String, String> = HashMap::new();

    // Separate epics and tasks
    let epics: Vec<&BeadIssue> = issues.iter().filter(|i| i.issue_type == "epic").collect();
    let tasks: Vec<&BeadIssue> = issues.iter().filter(|i| i.issue_type != "epic").collect();

    // Create epics first
    for epic in &epics {
        if skip_closed && epic.status == "closed" {
            skipped += 1;
            continue;
        }

        let quality_gates = extract_quality_gates_from_description(&epic.description);

        let events = engine
            .create_epic(CreateEpic {
                title: epic.title.clone(),
                description: epic.description.clone(),
                quality_gates,
            })
            .await
            .map_err(|e| format!("Failed to create epic '{}': {e}", epic.title))?;

        if let Some(RewindEvent::EpicCreated { epic_id, .. }) = events.first() {
            id_map.insert(epic.id.clone(), epic_id.to_string());
        }
        epics_created += 1;
    }

    // Build parent-child map: task_bead_id -> parent_epic_bead_id
    let mut parent_map: HashMap<String, String> = HashMap::new();
    // Build blocks map: task_bead_id -> [blocking_task_bead_ids]
    let mut blocks_map: HashMap<String, Vec<String>> = HashMap::new();

    for issue in issues {
        for dep in &issue.dependencies {
            if dep.dep_type == "parent-child" {
                // dep.issue_id is the child, dep.depends_on_id is the parent
                parent_map.insert(dep.issue_id.clone(), dep.depends_on_id.clone());
            } else if dep.dep_type == "blocks" {
                blocks_map
                    .entry(dep.issue_id.clone())
                    .or_default()
                    .push(dep.depends_on_id.clone());
            }
        }
    }

    // Create tasks
    for task in &tasks {
        if skip_closed && task.status == "closed" {
            skipped += 1;
            continue;
        }

        let criteria = extract_criteria_from_description(&task.description);

        // Resolve parent epic
        let epic_id = parent_map
            .get(&task.id)
            .and_then(|parent_bead_id| id_map.get(parent_bead_id))
            .map(crate::domain::ids::EpicId::new);

        // Resolve blocking dependencies
        let depends_on: Vec<TaskId> = blocks_map
            .get(&task.id)
            .map(|blockers| {
                blockers
                    .iter()
                    .filter_map(|bead_id| id_map.get(bead_id))
                    .map(TaskId::new)
                    .collect()
            })
            .unwrap_or_default();

        let events = engine
            .create_task(CreateTask {
                title: task.title.clone(),
                description: task.description.clone(),
                epic_id,
                acceptance_criteria: criteria,
                story_type: None,
                depends_on,
            })
            .await
            .map_err(|e| format!("Failed to create task '{}': {e}", task.title))?;

        if let Some(RewindEvent::TaskCreated { task_id, .. }) = events.first() {
            id_map.insert(task.id.clone(), task_id.to_string());
        }
        tasks_created += 1;
    }

    Ok(ImportResult {
        epics_created,
        tasks_created,
        skipped,
    })
}

/// A user story section parsed from a PRD markdown file.
#[derive(Debug, Clone)]
struct PrdStory {
    /// The US-xxx prefix extracted from the heading (e.g. "US-007-01").
    prefix: String,
    /// Full description text including acceptance criteria.
    description: String,
    /// Extracted acceptance criteria.
    criteria: Vec<AcceptanceCriterion>,
}

/// Parse a PRD markdown file and extract user story sections.
///
/// Looks for `### US-xxx:` headings and collects everything up to the next
/// heading of equal or higher level.
fn parse_prd_stories(prd_content: &str) -> Vec<PrdStory> {
    let mut stories = Vec::new();
    let mut current_prefix = String::new();
    let mut current_lines: Vec<&str> = Vec::new();

    for line in prd_content.lines() {
        if line.starts_with("### US-") || line.starts_with("### us-") {
            // Flush previous story
            if !current_prefix.is_empty() {
                let description = current_lines.join("\n");
                let criteria = extract_criteria_from_description(&description);
                stories.push(PrdStory {
                    prefix: current_prefix.clone(),
                    description,
                    criteria,
                });
            }

            // Extract US-xxx prefix from heading like "### US-007-01: RouterClient with failover [Backend]"
            let heading = line.trim_start_matches('#').trim();
            current_prefix = heading
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            current_lines.clear();
            current_lines.push(line);
        } else if line.starts_with("## ") && !current_prefix.is_empty() {
            // Higher-level heading — flush current story
            let description = current_lines.join("\n");
            let criteria = extract_criteria_from_description(&description);
            stories.push(PrdStory {
                prefix: current_prefix.clone(),
                description,
                criteria,
            });
            current_prefix.clear();
            current_lines.clear();
        } else if !current_prefix.is_empty() {
            current_lines.push(line);
        }
    }

    // Flush last story
    if !current_prefix.is_empty() {
        let description = current_lines.join("\n");
        let criteria = extract_criteria_from_description(&description);
        stories.push(PrdStory {
            prefix: current_prefix,
            description,
            criteria,
        });
    }

    stories
}

/// Try to find a PRD file in `tasks/` that matches the epic title.
///
/// Scans `tasks/*.md` for files whose content contains a heading matching the epic title.
fn find_prd_for_epic(epic_title: &str) -> Option<String> {
    let tasks_dir = Path::new("tasks");
    if !tasks_dir.is_dir() {
        return None;
    }

    let entries = std::fs::read_dir(tasks_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            // Match if the PRD contains a heading with the epic title or its PRD number
            // e.g. epic_title = "PRD-007: LLM Router..." → look for "PRD-007" in the file
            let prd_number = epic_title
                .split(':')
                .next()
                .unwrap_or("")
                .trim();
            if !prd_number.is_empty() && content.contains(prd_number) {
                return Some(content);
            }
        }
    }
    None
}

/// Import an epic and its child tasks from chronis into the engine.
///
/// Fetches the epic via `cn show`, then tries to find a matching PRD file in
/// `tasks/` to populate task descriptions and acceptance criteria. Falls back
/// to title-only import if no PRD is found.
pub async fn import_epic_from_chronis<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
    epic_id: &str,
    engine: &RewindEngine<B>,
) -> Result<ImportResult, String> {
    use crate::infrastructure::chronis::ChronisBridge;

    let epic = ChronisBridge::show_epic(epic_id)
        .map_err(|e| format!("Failed to fetch epic: {e}"))?;

    // Skip if epic is already done
    if epic.status == "done" || epic.status == "closed" {
        return Err(format!("Epic {} is already {}", epic.id, epic.status));
    }

    // Try to find a PRD file with story descriptions
    let prd_stories = find_prd_for_epic(&epic.title)
        .map(|content| parse_prd_stories(&content))
        .unwrap_or_default();

    let mut epics_created = 0;
    let mut tasks_created = 0;
    let mut skipped = 0;

    // Extract epic-level quality gates from PRD if available
    let quality_gates = find_prd_for_epic(&epic.title)
        .map(|content| extract_quality_gates_from_description(&content))
        .unwrap_or_default();

    let events = engine
        .create_epic(CreateEpic {
            title: epic.title.clone(),
            description: format!("Imported from chronis: {}", epic.id),
            quality_gates,
        })
        .await
        .map_err(|e| format!("Failed to create epic: {e}"))?;

    let rewind_epic_id = match events.first() {
        Some(RewindEvent::EpicCreated { epic_id, .. }) => epic_id.clone(),
        _ => return Err("Failed to get epic ID from create event".into()),
    };
    epics_created += 1;

    let mut id_map: HashMap<String, TaskId> = HashMap::new();

    for child in &epic.children {
        if child.status == "done" || child.status == "closed" {
            skipped += 1;
            continue;
        }

        // Match child task to PRD story by US-xxx prefix in the title
        let matched_story = prd_stories.iter().find(|story| {
            child.title.contains(&story.prefix)
        });

        let (description, criteria) = match matched_story {
            Some(story) => (story.description.clone(), story.criteria.clone()),
            None => {
                // Fall back to chronis show for description
                let desc = ChronisBridge::show_task(&child.id)
                    .ok()
                    .and_then(|detail| {
                        detail
                            .lines()
                            .find_map(|line| line.strip_prefix("description:"))
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default();
                let crit = extract_criteria_from_description(&desc);
                (desc, crit)
            }
        };

        let events = engine
            .create_task(CreateTask {
                title: child.title.clone(),
                description,
                epic_id: Some(rewind_epic_id.clone()),
                acceptance_criteria: criteria,
                story_type: None,
                depends_on: Vec::new(),
            })
            .await
            .map_err(|e| format!("Failed to create task '{}': {e}", child.title))?;

        if let Some(RewindEvent::TaskCreated { task_id, .. }) = events.first() {
            id_map.insert(child.id.clone(), task_id.clone());
        }
        tasks_created += 1;
    }

    Ok(ImportResult {
        epics_created,
        tasks_created,
        skipped,
    })
}

/// Import from a file path, auto-detecting format.
pub async fn import_file<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
    path: &Path,
    engine: &RewindEngine<B>,
    skip_closed: bool,
) -> Result<ImportResult, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "jsonl" => {
            let issues = parse_beads_jsonl(&content)?;
            import_beads(&issues, engine, skip_closed).await
        }
        "json" => {
            // Try parsing as a JSON array of issues
            let issues: Vec<BeadIssue> =
                serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {e}"))?;
            import_beads(&issues, engine, skip_closed).await
        }
        _ => Err(format!(
            "Unsupported file format: '.{ext}'. Supported: .jsonl (beads), .json"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSONL: &str = r#"{"id":"bd-abc","title":"Feature Epic","description":"Build the thing.\n\n## Epic Quality Gates\n- [ ] `cargo test` passes\n- [ ] `cargo clippy` passes","status":"open","priority":1,"issue_type":"epic","dependencies":[],"external_ref":"prd:./tasks/feature.md","created_at":"2026-03-03T11:10:57Z","created_by":"user","updated_at":"2026-03-03T11:10:57Z","source_repo":".","compaction_level":0,"original_size":0}
{"id":"bd-abc.1","title":"US-001: Add schema","description":"Add the database schema.\n\n## Acceptance Criteria\n- [ ] Migration file exists\n- [ ] Migration runs successfully","status":"open","priority":1,"issue_type":"task","dependencies":[{"issue_id":"bd-abc.1","depends_on_id":"bd-abc","type":"parent-child","created_at":"2026-03-03T11:11:38Z","created_by":"user"}],"created_at":"2026-03-03T11:11:38Z","created_by":"user","updated_at":"2026-03-03T11:11:38Z","source_repo":".","compaction_level":0,"original_size":0}
{"id":"bd-abc.2","title":"US-002: Backend endpoint","description":"Add the API endpoint.\n\n## Acceptance Criteria\n- [ ] GET /api/data returns 200\n- [ ] Response includes correct fields","status":"open","priority":2,"issue_type":"task","dependencies":[{"issue_id":"bd-abc.2","depends_on_id":"bd-abc","type":"parent-child","created_at":"2026-03-03T11:12:00Z","created_by":"user"},{"issue_id":"bd-abc.2","depends_on_id":"bd-abc.1","type":"blocks","created_at":"2026-03-03T11:12:30Z","created_by":"user"}],"created_at":"2026-03-03T11:12:00Z","created_by":"user","updated_at":"2026-03-03T11:12:00Z","source_repo":".","compaction_level":0,"original_size":0}
{"id":"bd-xyz","title":"Closed Epic","description":"Already done","status":"closed","priority":1,"issue_type":"epic","dependencies":[],"created_at":"2026-03-03T10:00:00Z","created_by":"user","updated_at":"2026-03-03T10:00:00Z","source_repo":".","compaction_level":0,"original_size":0}"#;

    #[test]
    fn parse_beads_jsonl_parses_all_lines() {
        let issues = parse_beads_jsonl(SAMPLE_JSONL).unwrap();
        assert_eq!(issues.len(), 4);
        assert_eq!(issues[0].issue_type, "epic");
        assert_eq!(issues[1].issue_type, "task");
        assert_eq!(issues[2].issue_type, "task");
        assert_eq!(issues[3].status, "closed");
    }

    #[test]
    fn parse_beads_jsonl_extracts_dependencies() {
        let issues = parse_beads_jsonl(SAMPLE_JSONL).unwrap();
        // US-001 has parent-child dep to epic
        assert_eq!(issues[1].dependencies.len(), 1);
        assert_eq!(issues[1].dependencies[0].dep_type, "parent-child");
        // US-002 has parent-child + blocks dep
        assert_eq!(issues[2].dependencies.len(), 2);
    }

    #[test]
    fn extract_criteria_from_description_works() {
        let desc = "Some intro text.\n\n## Acceptance Criteria\n- [ ] First thing\n- [x] Second thing\n- [ ] Third thing\n\nSome trailing text.";
        let criteria = extract_criteria_from_description(desc);
        assert_eq!(criteria.len(), 3);
        assert_eq!(criteria[0].description, "First thing");
        assert!(!criteria[0].checked);
        assert_eq!(criteria[1].description, "Second thing");
        assert!(criteria[1].checked);
        assert_eq!(criteria[2].description, "Third thing");
        assert!(!criteria[2].checked);
    }

    #[test]
    fn extract_criteria_empty_description() {
        let criteria = extract_criteria_from_description("No checkboxes here.");
        assert!(criteria.is_empty());
    }

    #[test]
    fn extract_quality_gates_from_description_works() {
        let desc = "Build the thing.\n\n## Epic Quality Gates\n- [ ] `cargo test` passes\n- [ ] `cargo clippy -- -D warnings` passes";
        let gates = extract_quality_gates_from_description(desc);
        assert_eq!(gates.len(), 2);
        assert_eq!(gates[0].command, "cargo test");
        assert_eq!(gates[1].command, "cargo clippy -- -D warnings");
    }

    #[test]
    fn extract_quality_gates_no_gates() {
        let gates = extract_quality_gates_from_description("Just a description.");
        assert!(gates.is_empty());
    }

    #[tokio::test]
    async fn import_beads_creates_epic_and_tasks() {
        let issues = parse_beads_jsonl(SAMPLE_JSONL).unwrap();
        let engine = RewindEngine::in_memory().await;

        let result = import_beads(&issues, &engine, false).await.unwrap();
        assert_eq!(result.epics_created, 2); // both epics (open + closed)
        assert_eq!(result.tasks_created, 2);
        assert_eq!(result.skipped, 0);

        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        assert_eq!(backlog.task_count(), 2);
    }

    #[tokio::test]
    async fn import_beads_skips_closed() {
        let issues = parse_beads_jsonl(SAMPLE_JSONL).unwrap();
        let engine = RewindEngine::in_memory().await;

        let result = import_beads(&issues, &engine, true).await.unwrap();
        assert_eq!(result.epics_created, 1); // only the open epic
        assert_eq!(result.tasks_created, 2);
        assert_eq!(result.skipped, 1); // the closed epic
    }

    #[tokio::test]
    async fn import_beads_links_parent_epic() {
        let issues = parse_beads_jsonl(SAMPLE_JSONL).unwrap();
        let engine = RewindEngine::in_memory().await;

        let _ = import_beads(&issues, &engine, true).await.unwrap();

        // Check that tasks are linked to the epic
        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        let tasks: Vec<_> = backlog.tasks.values().collect();
        assert!(tasks.iter().all(|t| t.epic_id.is_some()));
    }

    #[tokio::test]
    async fn import_beads_resolves_blocking_deps() {
        let issues = parse_beads_jsonl(SAMPLE_JSONL).unwrap();
        let engine = RewindEngine::in_memory().await;

        let _ = import_beads(&issues, &engine, true).await.unwrap();

        // US-002 should depend on US-001
        let backlog = engine.backlog();
        let backlog = backlog.read().await;

        // Find US-002 (Backend endpoint)
        let us002 = backlog
            .tasks
            .values()
            .find(|t| t.title.contains("Backend endpoint"))
            .expect("US-002 should exist");

        assert_eq!(us002.depends_on.len(), 1, "US-002 should have 1 dependency");
    }

    #[tokio::test]
    async fn import_beads_extracts_criteria() {
        let issues = parse_beads_jsonl(SAMPLE_JSONL).unwrap();
        let engine = RewindEngine::in_memory().await;

        let _ = import_beads(&issues, &engine, true).await.unwrap();

        let backlog = engine.backlog();
        let backlog = backlog.read().await;

        let us001 = backlog
            .tasks
            .values()
            .find(|t| t.title.contains("Add schema"))
            .expect("US-001 should exist");

        assert_eq!(us001.acceptance_criteria.len(), 2);
        assert_eq!(
            us001.acceptance_criteria[0].description,
            "Migration file exists"
        );
    }

    #[test]
    fn parse_prd_stories_extracts_user_stories() {
        let prd = r#"# PRD: Test Feature

## Overview
Some overview text.

## User Stories

### US-001: Add schema [Schema]
As a developer, I want a schema.

**Acceptance Criteria:**
- [ ] Migration file exists
- [ ] Migration runs successfully

### US-002: Add endpoint [Backend]
As a developer, I want an endpoint.

**Acceptance Criteria:**
- [ ] GET /api/data returns 200
- [ ] Response has correct fields

## Non-Goals
Something.
"#;

        let stories = parse_prd_stories(prd);
        assert_eq!(stories.len(), 2);

        assert_eq!(stories[0].prefix, "US-001");
        assert!(stories[0].description.contains("Migration file exists"));
        assert_eq!(stories[0].criteria.len(), 2);
        assert_eq!(stories[0].criteria[0].description, "Migration file exists");

        assert_eq!(stories[1].prefix, "US-002");
        assert_eq!(stories[1].criteria.len(), 2);
        assert_eq!(stories[1].criteria[0].description, "GET /api/data returns 200");
    }

    #[test]
    fn parse_prd_stories_handles_no_stories() {
        let prd = "# PRD: Empty\n\nJust overview.";
        let stories = parse_prd_stories(prd);
        assert!(stories.is_empty());
    }

    #[test]
    fn parse_prd_stories_matches_compound_prefixes() {
        let prd = "### US-007-01: RouterClient with failover [Backend]\nDescription here.\n\n- [ ] Criterion one\n- [ ] Criterion two\n";
        let stories = parse_prd_stories(prd);
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0].prefix, "US-007-01");
        assert_eq!(stories[0].criteria.len(), 2);
    }
}
