use std::path::Path;
use std::sync::Arc;

use ralph_core::application::commands::EndSession;
use ralph_core::application::scheduler::pick_runnable_tasks;
use ralph_core::domain::events::RalphEvent;
use ralph_core::domain::ids::TaskId;
use ralph_core::infrastructure::agent::AgentWorker;
use ralph_core::infrastructure::engine::RalphEngine;

use crate::config::RalphConfig;

const DATA_DIR: &str = ".ralph/data";
const CONFIG_FILE: &str = ".ralph/ralph.toml";

pub async fn execute(
    task_filter: Option<String>,
    dry_run: bool,
    max_concurrent_override: Option<usize>,
) -> Result<(), String> {
    if !Path::new(".ralph").exists() {
        return Err("No ralph project found. Run `ralph init` first.".into());
    }

    let config = RalphConfig::load(Path::new(CONFIG_FILE))?;
    let max_concurrent = max_concurrent_override.unwrap_or(config.agents.max_concurrent);

    let engine = RalphEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    engine
        .rebuild_projections()
        .await
        .map_err(|e| e.to_string())?;

    // Determine tasks to run
    let tasks_to_run: Vec<(TaskId, String)> = {
        let backlog = engine.backlog();
        let backlog = backlog.read().await;

        if let Some(ref tid) = task_filter {
            match backlog.tasks.get(tid) {
                Some(task) => {
                    if task.status.to_string() != "pending" {
                        return Err(format!(
                            "Task {tid} is not pending (status: {})",
                            task.status
                        ));
                    }
                    vec![(task.task_id.clone(), task.title.clone())]
                }
                None => return Err(format!("Task not found: {tid}")),
            }
        } else {
            let runnable = pick_runnable_tasks(&backlog, max_concurrent);
            runnable
                .iter()
                .map(|t| (t.task_id.clone(), t.title.clone()))
                .collect()
        }
    };

    if tasks_to_run.is_empty() {
        println!("No pending tasks to run.");
        return Ok(());
    }

    if dry_run {
        println!("[dry run] Would execute {} task(s):", tasks_to_run.len());
        for (i, (id, title)) in tasks_to_run.iter().enumerate() {
            println!("  {}. {} ({})", i + 1, title, id);
        }
        return Ok(());
    }

    // Start session
    let session_events = engine.start_session().await.map_err(|e| e.to_string())?;
    let session_id = match &session_events[0] {
        RalphEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => return Err("Unexpected event type".into()),
    };
    println!("Session started: {session_id}");

    let total = tasks_to_run.len();
    let mut completed = 0usize;
    let mut failed = 0usize;

    // Execute tasks (sequential for Phase 1, concurrent via JoinSet in Phase 2)
    let engine = Arc::new(engine);
    for (i, (task_id, title)) in tasks_to_run.iter().enumerate() {
        let worker = AgentWorker::new();
        print!("[{}/{}] Executing: {}... ", i + 1, total, title);

        match worker
            .execute_task(task_id.clone(), title, engine.as_ref())
            .await
        {
            Ok(_) => {
                println!("done");
                completed += 1;
            }
            Err(e) => {
                println!("FAILED ({})", e);
                failed += 1;
            }
        }
    }

    // End session
    let _ = engine
        .end_session(EndSession { session_id })
        .await
        .map_err(|e| e.to_string())?;

    println!(
        "Session complete: {} task(s) executed ({} passed, {} failed)",
        total, completed, failed
    );

    if failed > 0 {
        return Err(format!("{failed} task(s) failed"));
    }

    Ok(())
}
