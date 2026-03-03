use std::path::Path;

use ralph_core::application::status::build_summary;
use ralph_core::infrastructure::engine::RalphEngine;

const DATA_DIR: &str = ".ralph/data";

pub async fn execute(json: bool) -> Result<(), String> {
    if !Path::new(".ralph").exists() {
        return Err("No ralph project found. Run `ralph init` first.".into());
    }

    let engine = RalphEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    engine.rebuild_projections().await.map_err(|e| e.to_string())?;

    let backlog = engine.backlog();
    let backlog = backlog.read().await;
    let epic_progress = engine.epic_progress();
    let epic_progress = epic_progress.read().await;

    let summary = build_summary(&backlog, &epic_progress);

    if json {
        let output = serde_json::to_string_pretty(&summary)
            .map_err(|e| format!("Failed to serialize: {e}"))?;
        println!("{output}");
    } else {
        println!("Tasks: {} total", summary.total_tasks);

        let statuses = ["pending", "assigned", "in-progress", "completed", "failed", "blocked"];
        for status in &statuses {
            let count = summary.by_status.get(*status).unwrap_or(&0);
            if *count > 0 {
                println!("  {:<14}{}", format!("{status}:"), count);
            }
        }

        if !summary.epics.is_empty() {
            println!();
            println!("Epics:");
            for epic in &summary.epics {
                let pct = if epic.total_tasks == 0 {
                    0
                } else {
                    (epic.completed_tasks * 100) / epic.total_tasks
                };
                let filled = pct / 10;
                let empty = 10 - filled;
                let bar = format!(
                    "[{}{}]",
                    "#".repeat(filled),
                    ".".repeat(empty)
                );
                let status = if epic.is_completed { " (done)" } else { "" };
                println!(
                    "  {}  {} {}% ({}/{}){status}",
                    epic.title, bar, pct, epic.completed_tasks, epic.total_tasks
                );
            }
        }
    }

    Ok(())
}
