use std::path::Path;
use std::sync::Arc;

use rewind_cn_core::application::commands::EndSession;
use rewind_cn_core::application::scheduler::pick_runnable_tasks;
use rewind_cn_core::domain::events::RewindEvent;
use rewind_cn_core::domain::ids::TaskId;
use rewind_cn_core::infrastructure::agent::AgentWorker;
use rewind_cn_core::infrastructure::chronis::ChronisBridge;
use rewind_cn_core::infrastructure::engine::RewindEngine;
use rewind_cn_core::infrastructure::claude_code::{ClaudeCodeConfig, ClaudeCodeExecutor};
use rewind_cn_core::infrastructure::importer::import_epic_from_chronis;
use rewind_cn_core::infrastructure::llm::{create_coder_client, create_evaluator_client};
use rewind_cn_core::infrastructure::orchestrator::Orchestrator;

use rewind_cn_core::infrastructure::telemetry::{TelemetryClient, TelemetryClientConfig};

use crate::config::RewindConfig;

const DATA_DIR: &str = ".rewind/data";
const CONFIG_FILE: &str = ".rewind/rewind.toml";

pub async fn execute(
    task_filter: Option<String>,
    dry_run: bool,
    max_concurrent_override: Option<usize>,
    parallel: bool,
    mut use_tui: bool,
) -> Result<(), String> {
    if !Path::new(".rewind").exists() {
        return Err("No rewind project found. Run `rewind init` first.".into());
    }

    let config = RewindConfig::load(Path::new(CONFIG_FILE))?;
    let max_concurrent = max_concurrent_override.unwrap_or(config.execution.max_concurrent);

    // Initialize telemetry client
    let telemetry_id = std::fs::read_to_string(".rewind/telemetry_id").unwrap_or_default();
    let telemetry = TelemetryClient::new(TelemetryClientConfig {
        enabled: config.telemetry.enabled,
        posthog_key: config.telemetry.posthog_key.clone(),
        posthog_host: config.telemetry.posthog_host.clone(),
        distinct_id: telemetry_id.trim().to_string(),
    });

    let engine = RewindEngine::load(DATA_DIR)
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
        // No internal tasks — check chronis for epics to import
        if !ChronisBridge::is_available() {
            println!("No pending tasks to run.");
            println!("Hint: Install chronis (cn CLI) to browse and import epics.");
            return Ok(());
        }

        let epics = ChronisBridge::list_epics().map_err(|e| e.to_string())?;
        if epics.is_empty() {
            println!("No pending tasks and no chronis epics available.");
            return Ok(());
        }

        // Fetch child tasks for each epic (for the detail panel)
        let mut browse_entries = Vec::new();
        for epic in &epics {
            let children = match ChronisBridge::show_epic(&epic.id) {
                Ok(detail) => detail.children,
                Err(_) => Vec::new(),
            };
            browse_entries.push(crate::tui::EpicEntry {
                id: epic.id.clone(),
                title: epic.title.clone(),
                priority: epic.priority.clone(),
                status: epic.status.clone(),
                children,
            });
        }

        // Launch epic browser TUI
        let selected = crate::tui::run_epic_browser(browse_entries)
            .await
            .map_err(|e| format!("TUI error: {e}"))?;

        let Some(epic_id) = selected else {
            return Ok(());
        };

        // Auto-enable TUI for execution after interactive selection
        use_tui = true;

        eprintln!("Importing epic {epic_id} from chronis...");
        let import_result = import_epic_from_chronis(&epic_id, &engine)
            .await
            .map_err(|e| format!("Import failed: {e}"))?;

        eprintln!(
            "Imported {} epic(s), {} task(s) ({} skipped)",
            import_result.epics_created, import_result.tasks_created, import_result.skipped,
        );

        // Rebuild projections after import
        engine
            .rebuild_projections()
            .await
            .map_err(|e| e.to_string())?;

        // Re-check for runnable tasks
        let backlog = engine.backlog();
        let backlog_read = backlog.read().await;
        let new_runnable = pick_runnable_tasks(&backlog_read, max_concurrent);
        if new_runnable.is_empty() {
            println!("No runnable tasks after import (all may be blocked or done).");
            return Ok(());
        }
        drop(backlog_read);
    }

    if dry_run {
        println!("[dry run] Would execute {} task(s):", tasks_to_run.len());
        for (i, (id, title)) in tasks_to_run.iter().enumerate() {
            println!("  {}. {} ({})", i + 1, title, id);
        }
        return Ok(());
    }

    // Check if we should use the orchestrator (LLM agent execution)
    if let Some(ref agent_config) = config.agent {
        eprintln!(
            "Using LLM orchestrator (coder: {}, evaluator: {})",
            agent_config.coder.model, agent_config.evaluator.model
        );
        let evaluator_client = create_evaluator_client(agent_config).map_err(|e| e.to_string())?;
        let work_dir = std::env::current_dir().map_err(|e| e.to_string())?;

        let orchestrator = if agent_config.coder_backend == "claude-code" {
            eprintln!("Using Claude Code CLI backend (no API key needed for coder)");
            let cc_config = ClaudeCodeConfig {
                model: Some(agent_config.coder.model.clone()),
                max_turns: None,
                skip_permissions: true,
            };
            Orchestrator::with_coder(
                Box::new(ClaudeCodeExecutor::new(cc_config)),
                evaluator_client,
                agent_config.clone(),
                work_dir,
                config.execution.timeout_secs,
                config.execution.max_retries,
            )
        } else {
            let coder_client = create_coder_client(agent_config).map_err(|e| e.to_string())?;
            Orchestrator::new(
                coder_client,
                evaluator_client,
                agent_config.clone(),
                work_dir,
                config.execution.timeout_secs,
                config.execution.max_retries,
            )
        };

        // Start session
        let session_events = engine.start_session().await.map_err(|e| e.to_string())?;
        let session_id = match &session_events[0] {
            RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
            _ => return Err("Unexpected event type".into()),
        };
        eprintln!("Session started: {session_id}");

        telemetry
            .capture_simple(
                "rewind.session.started",
                &[
                    ("version", env!("CARGO_PKG_VERSION")),
                    ("os", std::env::consts::OS),
                    ("arch", std::env::consts::ARCH),
                ],
            )
            .await;

        let session_start = std::time::Instant::now();

        // Optionally start TUI dashboard — seed with current backlog state
        // so tasks imported before the TUI subscribes are visible immediately.
        let tui_handle = if use_tui {
            let event_rx = engine.subscribe();
            let backlog_snapshot = {
                let bl = engine.backlog();
                let bl = bl.read().await;
                bl.clone()
            };
            let epic_snapshot = {
                let ep = engine.epic_progress();
                let ep = ep.read().await;
                ep.clone()
            };
            Some(tokio::spawn(async move {
                if let Err(e) = crate::tui::run_dashboard(
                    event_rx,
                    Some(&backlog_snapshot),
                    Some(&epic_snapshot),
                )
                .await
                {
                    eprintln!("TUI error: {e}");
                }
            }))
        } else {
            None
        };

        let (completed, failed) = if parallel {
            eprintln!("Parallel mode: using git worktrees for isolation");
            let orchestrator = Arc::new(orchestrator);
            let engine = Arc::new(engine);
            let result = orchestrator
                .execute_parallel(engine.clone(), max_concurrent)
                .await
                .map_err(|e| e.to_string())?;

            let _ = engine
                .end_session(EndSession {
                    session_id: session_id.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;

            result
        } else {
            let result = orchestrator
                .execute_runnable(&engine, max_concurrent)
                .await
                .map_err(|e| e.to_string())?;

            let _ = engine
                .end_session(EndSession { session_id })
                .await
                .map_err(|e| e.to_string())?;

            result
        };

        let duration_ms = session_start.elapsed().as_millis().to_string();
        telemetry
            .capture_simple(
                "rewind.session.completed",
                &[
                    ("version", env!("CARGO_PKG_VERSION")),
                    ("tasks_completed", &completed.to_string()),
                    ("tasks_failed", &failed.to_string()),
                    ("duration_ms", &duration_ms),
                    ("parallel", if parallel { "true" } else { "false" }),
                ],
            )
            .await;
        telemetry.flush().await;

        // Wait for TUI to finish (user presses 'q')
        if let Some(handle) = tui_handle {
            let _ = handle.await;
        }

        println!(
            "Session complete: {} task(s) executed ({} passed, {} failed)",
            completed + failed,
            completed,
            failed
        );

        if failed > 0 {
            return Err(format!("{failed} task(s) failed"));
        }

        return Ok(());
    }

    // Fallback: Phase 1 mock execution
    let session_events = engine.start_session().await.map_err(|e| e.to_string())?;
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => return Err("Unexpected event type".into()),
    };
    println!("Session started: {session_id}");

    let total = tasks_to_run.len();
    let mut completed = 0usize;
    let mut failed = 0usize;

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
