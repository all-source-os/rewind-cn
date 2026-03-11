use std::path::Path;

use rewind_cn_core::infrastructure::engine::RewindEngine;

const DATA_DIR: &str = ".rewind/data";

pub async fn execute(
    query_name: String,
    json: bool,
    epic_filter: Option<String>,
) -> Result<(), String> {
    if !Path::new(".rewind").exists() {
        return Err("No rewind project found. Run `rewind init` first.".into());
    }

    if query_name == "list" {
        println!("Available queries:");
        println!("  task-summary     Per-task duration, status, criteria progress");
        println!("  epic-summary     Per-epic completion %, gate results");
        println!("  tool-usage       Tool call frequency ranked by count");
        println!("  session-history  Session timeline with task counts");
        return Ok(());
    }

    let engine = RewindEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    engine
        .rebuild_projections()
        .await
        .map_err(|e| e.to_string())?;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    match query_name.as_str() {
        "task-summary" => {
            let tasks = analytics.task_summary(epic_filter.as_deref());
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&tasks).map_err(|e| e.to_string())?
                );
            } else {
                println!(
                    "{:<40} {:<12} {:>10} {:>8} {:>10}",
                    "TITLE", "STATUS", "DURATION", "TOOLS", "CRITERIA"
                );
                println!("{}", "-".repeat(84));
                for t in &tasks {
                    let duration = t
                        .duration_secs
                        .map(|d| format!("{:.1}s", d))
                        .unwrap_or_else(|| "-".into());
                    let criteria = format!("{}/{}", t.criteria_checked, t.criteria_total);
                    let status = format!("{:?}", t.outcome);
                    let title = if t.title.len() > 38 {
                        format!("{}…", &t.title[..37])
                    } else {
                        t.title.clone()
                    };
                    println!(
                        "{:<40} {:<12} {:>10} {:>8} {:>10}",
                        title, status, duration, t.tool_call_count, criteria
                    );
                }
            }
        }
        "epic-summary" => {
            let epics = analytics.epic_summary();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&epics).map_err(|e| e.to_string())?
                );
            } else {
                println!(
                    "{:<40} {:>8} {:>8} {:>8} {:>8} {:>10}",
                    "TITLE", "TOTAL", "DONE", "FAIL", "GATES", "STATUS"
                );
                println!("{}", "-".repeat(86));
                for e in &epics {
                    let pct = if e.total_tasks > 0 {
                        format!("{}%", e.completed * 100 / e.total_tasks)
                    } else {
                        "0%".into()
                    };
                    let gates = format!("{}/{}", e.gates_passed, e.gates_total);
                    let status = if e.is_completed { "done" } else { "open" };
                    let title = if e.title.len() > 38 {
                        format!("{}…", &e.title[..37])
                    } else {
                        e.title.clone()
                    };
                    println!(
                        "{:<40} {:>8} {:>8} {:>8} {:>8} {:>10}",
                        title, e.total_tasks, pct, e.failed, gates, status
                    );
                }
            }
        }
        "tool-usage" => {
            let usage = analytics.tool_usage();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&usage).map_err(|e| e.to_string())?
                );
            } else {
                println!("{:<30} {:>10}", "TOOL", "CALLS");
                println!("{}", "-".repeat(42));
                for u in &usage {
                    println!("{:<30} {:>10}", u.tool_name, u.call_count);
                }
            }
        }
        "session-history" => {
            let sessions = analytics.session_history();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&sessions).map_err(|e| e.to_string())?
                );
            } else {
                println!("{:<38} {:<22} {:>10}", "SESSION", "STARTED", "DURATION");
                println!("{}", "-".repeat(72));
                for s in &sessions {
                    let duration = s
                        .duration_secs
                        .map(|d| format!("{:.1}s", d))
                        .unwrap_or_else(|| "running".into());
                    let started = s.started_at.format("%Y-%m-%d %H:%M:%S").to_string();
                    let id_str = s.session_id.to_string();
                    let id = if id_str.len() > 8 {
                        &id_str[..8]
                    } else {
                        &id_str
                    };
                    println!("{:<38} {:<22} {:>10}", id, started, duration);
                }
            }
        }
        _ => {
            return Err(format!(
                "Unknown query: '{query_name}'. Run `rewind query list` to see available queries."
            ));
        }
    }

    Ok(())
}
