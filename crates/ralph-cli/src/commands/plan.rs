use std::io::Read;
use std::path::{Path, PathBuf};

use ralph_core::application::commands::{CreateEpic, CreateTask};
use ralph_core::application::planning::passthrough_plan;
use ralph_core::domain::events::RalphEvent;
use ralph_core::infrastructure::engine::RalphEngine;

const DATA_DIR: &str = ".ralph/data";

pub async fn execute(
    description: Option<String>,
    file: Option<PathBuf>,
    dry_run: bool,
) -> Result<(), String> {
    if !Path::new(".ralph").exists() {
        return Err("No ralph project found. Run `ralph init` first.".into());
    }

    let input = resolve_input(description, file)?;
    let plan = passthrough_plan(&input);

    if dry_run {
        println!("[dry run]");
    }

    println!("Epic: {}", plan.epic_title);
    println!();
    for (i, task) in plan.tasks.iter().enumerate() {
        println!("  {}. {}", i + 1, task.title);
    }
    println!();

    if dry_run {
        println!("Dry run — no events persisted.");
        return Ok(());
    }

    let engine = RalphEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    // Create epic
    let epic_events = engine
        .create_epic(CreateEpic {
            title: plan.epic_title.clone(),
            description: plan.epic_description.clone(),
        })
        .await
        .map_err(|e| e.to_string())?;

    let epic_id = match &epic_events[0] {
        RalphEvent::EpicCreated { epic_id, .. } => epic_id.clone(),
        _ => return Err("Unexpected event type".into()),
    };

    // Create tasks
    for task in &plan.tasks {
        engine
            .create_task(CreateTask {
                title: task.title.clone(),
                description: task.description.clone(),
                epic_id: Some(epic_id.clone()),
            })
            .await
            .map_err(|e| e.to_string())?;
    }

    println!(
        "Created 1 epic with {} task{}.",
        plan.tasks.len(),
        if plan.tasks.len() == 1 { "" } else { "s" }
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

    // Read from stdin
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("Failed to read stdin: {e}"))?;

    if input.trim().is_empty() {
        return Err("No input provided. Usage: ralph plan \"description\" or ralph plan -f file.md".into());
    }

    Ok(input)
}
