use std::path::PathBuf;

use chrono::Utc;
use tracing::{debug, info, warn};

use crate::application::commands::{AssignTask, CompleteTask, FailTask, StartTask};
use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;
use crate::domain::ids::{AgentId, TaskId};
use crate::domain::model::TaskView;
use crate::infrastructure::chronis::ChronisBridge;
use crate::infrastructure::coder::{CoderAgent, ToolCallRecord};
use crate::infrastructure::engine::RewindEngine;
use crate::infrastructure::evaluator::EvaluatorAgent;
use crate::infrastructure::llm::AgentConfig;

use rig::providers::anthropic;

/// Orchestrator runs the SELECT → PROMPT → EXECUTE → EVALUATE loop.
pub struct Orchestrator {
    coder: CoderAgent,
    evaluator: EvaluatorAgent,
    agent_id: AgentId,
    work_dir: PathBuf,
    timeout_secs: u64,
    max_retries: u32,
    use_chronis: bool,
}

impl Orchestrator {
    /// Create a new orchestrator from config.
    pub fn new(
        client: anthropic::Client,
        config: AgentConfig,
        work_dir: PathBuf,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Self {
        let use_chronis = ChronisBridge::is_available();
        if use_chronis {
            debug!("Chronis bridge enabled");
        }

        Self {
            coder: CoderAgent::new(client.clone(), config.clone()),
            evaluator: EvaluatorAgent::new(client, config),
            agent_id: AgentId::generate(),
            work_dir,
            timeout_secs,
            max_retries,
            use_chronis,
        }
    }

    /// Create an orchestrator without chronis (for tests).
    #[cfg(test)]
    pub fn without_chronis(
        client: anthropic::Client,
        config: AgentConfig,
        work_dir: PathBuf,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Self {
        Self {
            coder: CoderAgent::new(client.clone(), config.clone()),
            evaluator: EvaluatorAgent::new(client, config),
            agent_id: AgentId::generate(),
            work_dir,
            timeout_secs,
            max_retries,
            use_chronis: false,
        }
    }

    /// Execute a single task through the full SELECT → PROMPT → EXECUTE → EVALUATE loop.
    ///
    /// Returns the tool call records and whether the task passed evaluation.
    pub async fn execute_task<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        task: &TaskView,
        engine: &RewindEngine<B>,
    ) -> Result<bool, RewindError> {
        let task_id = &task.task_id;
        let task_id_str = task_id.to_string();

        // Chronis claim (best-effort)
        if self.use_chronis {
            match ChronisBridge::claim(&task_id_str) {
                Ok(ack) => debug!("Chronis claim: {ack}"),
                Err(e) => warn!("Chronis claim failed (continuing): {e}"),
            }
        }

        // ASSIGN
        engine
            .assign_task(AssignTask {
                task_id: task_id.clone(),
                agent_id: self.agent_id.clone(),
            })
            .await?;

        // START
        engine
            .start_task(StartTask {
                task_id: task_id.clone(),
            })
            .await?;

        info!("Executing: {}", task.title);

        // EXECUTE: run coder agent
        let (tool_calls, agent_output) = self
            .coder
            .execute_task(
                &task.title,
                &task.description,
                &task.acceptance_criteria,
                self.work_dir.clone(),
                self.timeout_secs,
            )
            .await?;

        // Record tool calls as events
        for call in &tool_calls {
            let _ = engine
                .append_events(vec![RewindEvent::AgentToolCall {
                    task_id: task_id.clone(),
                    tool_name: call.tool_name.clone(),
                    args_summary: call.args_summary.clone(),
                    result_summary: call.result_summary.clone(),
                    called_at: Utc::now(),
                }])
                .await;
        }

        // EVALUATE: run evaluator agent
        let eval_result = self
            .evaluator
            .evaluate(
                &task.description,
                &task.acceptance_criteria,
                &tool_calls,
                &agent_output,
            )
            .await?;

        if eval_result.passed {
            engine
                .complete_task(CompleteTask {
                    task_id: task_id.clone(),
                })
                .await?;

            // Mark checked criteria
            for cr in &eval_result.criteria_results {
                if cr.passed {
                    let _ = engine
                        .append_events(vec![RewindEvent::CriterionChecked {
                            task_id: task_id.clone(),
                            criterion_index: cr.index,
                            checked_at: Utc::now(),
                        }])
                        .await;
                }
            }

            if self.use_chronis {
                match ChronisBridge::done(&task_id_str) {
                    Ok(ack) => debug!("Chronis done: {ack}"),
                    Err(e) => warn!("Chronis done failed: {e}"),
                }
            }

            Ok(true)
        } else {
            let reason = eval_result.summary.clone();
            engine
                .fail_task(FailTask {
                    task_id: task_id.clone(),
                    reason: reason.clone(),
                })
                .await?;

            if self.use_chronis {
                let _ = ChronisBridge::fail(&task_id_str, &reason);
            }

            Ok(false)
        }
    }

    /// Execute all runnable tasks up to max_concurrent.
    ///
    /// Returns (completed_count, failed_count).
    pub async fn execute_runnable<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        engine: &RewindEngine<B>,
        max_concurrent: usize,
    ) -> Result<(usize, usize), RewindError> {
        use crate::application::scheduler::pick_runnable_tasks;

        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut retry_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        loop {
            // Rebuild projections to see current state
            engine.rebuild_projections().await?;

            let tasks: Vec<TaskView> = {
                let backlog = engine.backlog();
                let backlog = backlog.read().await;
                pick_runnable_tasks(&backlog, max_concurrent)
                    .into_iter()
                    .cloned()
                    .collect()
            };

            if tasks.is_empty() {
                break;
            }

            let total = tasks.len();
            for (i, task) in tasks.iter().enumerate() {
                eprint!(
                    "[{}/{}] Executing: {}... ",
                    completed + failed + i + 1,
                    completed + failed + total,
                    task.title
                );

                match self.execute_task(task, engine).await {
                    Ok(true) => {
                        eprintln!("done");
                        completed += 1;
                    }
                    Ok(false) => {
                        let retries = retry_counts.entry(task.task_id.to_string()).or_insert(0);
                        *retries += 1;

                        if *retries < self.max_retries {
                            eprintln!("FAILED (retry {}/{})", retries, self.max_retries);
                            // Task was marked failed, would need to be re-queued
                            // For now, count as failed
                        } else {
                            eprintln!("FAILED (max retries reached)");
                        }
                        failed += 1;
                    }
                    Err(e) => {
                        eprintln!("ERROR: {e}");
                        failed += 1;
                    }
                }
            }
        }

        Ok((completed, failed))
    }

    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }
}

/// Emit tool call records as events (utility for callers).
pub fn tool_calls_to_events(task_id: &TaskId, records: &[ToolCallRecord]) -> Vec<RewindEvent> {
    records
        .iter()
        .map(|r| RewindEvent::AgentToolCall {
            task_id: task_id.clone(),
            tool_name: r.tool_name.clone(),
            args_summary: r.args_summary.clone(),
            result_summary: r.result_summary.clone(),
            called_at: Utc::now(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_calls_to_events_converts() {
        let task_id = TaskId::new("t-1");
        let records = vec![
            ToolCallRecord {
                tool_name: "read_file".into(),
                args_summary: "main.rs".into(),
                result_summary: "100 bytes".into(),
            },
            ToolCallRecord {
                tool_name: "run_command".into(),
                args_summary: "cargo test".into(),
                result_summary: "exit 0".into(),
            },
        ];

        let events = tool_calls_to_events(&task_id, &records);
        assert_eq!(events.len(), 2);

        match &events[0] {
            RewindEvent::AgentToolCall {
                tool_name,
                args_summary,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(args_summary, "main.rs");
            }
            _ => panic!("Expected AgentToolCall"),
        }
    }
}
