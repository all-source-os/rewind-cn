use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};
use std::time::Instant;

use crate::application::commands::{AssignTask, CompleteTask, FailTask, StartTask};
use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;
use crate::domain::ids::{AgentId, SessionId, TaskId};
use crate::domain::model::TaskView;
use crate::infrastructure::chronis::ChronisBridge;
use crate::infrastructure::coder::{CoderAgent, PromptContext, ToolCallRecord};
use crate::infrastructure::engine::RewindEngine;
use crate::infrastructure::evaluator::EvaluatorAgent;
use crate::infrastructure::llm::{AgentConfig, ProviderClient};
use crate::infrastructure::worktree::WorktreeManager;

/// Orchestrator runs the SELECT → PROMPT → EXECUTE → EVALUATE loop.
pub struct Orchestrator {
    coder: CoderAgent,
    evaluator: EvaluatorAgent,
    agent_id: AgentId,
    work_dir: PathBuf,
    timeout_secs: u64,
    max_retries: u32,
    use_chronis: bool,
    epic_name: Option<String>,
    project_context: Option<String>,
    prompt_template_path: Option<PathBuf>,
}

impl Orchestrator {
    /// Create a new orchestrator from config.
    pub fn new(
        coder_client: ProviderClient,
        evaluator_client: ProviderClient,
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
            coder: CoderAgent::new(coder_client, config.clone()),
            evaluator: EvaluatorAgent::new(evaluator_client, config),
            agent_id: AgentId::generate(),
            work_dir,
            timeout_secs,
            max_retries,
            use_chronis,
            epic_name: None,
            project_context: None,
            prompt_template_path: None,
        }
    }

    /// Set the epic name for prompt context.
    pub fn with_epic_name(mut self, name: String) -> Self {
        self.epic_name = Some(name);
        self
    }

    /// Set the project context for prompt context.
    pub fn with_project_context(mut self, ctx: String) -> Self {
        self.project_context = Some(ctx);
        self
    }

    /// Set a custom prompt template path.
    pub fn with_prompt_template_path(mut self, path: PathBuf) -> Self {
        self.prompt_template_path = Some(path);
        self
    }

    /// Create an orchestrator without chronis (for tests).
    #[cfg(test)]
    pub fn without_chronis(
        coder_client: ProviderClient,
        evaluator_client: ProviderClient,
        config: AgentConfig,
        work_dir: PathBuf,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Self {
        Self {
            coder: CoderAgent::new(coder_client, config.clone()),
            evaluator: EvaluatorAgent::new(evaluator_client, config),
            agent_id: AgentId::generate(),
            work_dir,
            timeout_secs,
            max_retries,
            use_chronis: false,
            epic_name: None,
            project_context: None,
            prompt_template_path: None,
        }
    }

    /// Execute a single task through the full SELECT → PROMPT → EXECUTE → EVALUATE loop.
    ///
    /// Returns whether the task passed evaluation.
    #[hotpath::measure]
    pub async fn execute_task<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        task: &TaskView,
        engine: &RewindEngine<B>,
    ) -> Result<bool, RewindError> {
        self.execute_task_impl(task, engine, None).await
    }

    /// Execute all runnable tasks up to max_concurrent.
    ///
    /// Returns (completed_count, failed_count).
    #[hotpath::measure]
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

    /// Execute a single task in a specific working directory (for worktree isolation).
    #[hotpath::measure]
    pub async fn execute_task_in_dir<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        task: &TaskView,
        engine: &RewindEngine<B>,
        work_dir: PathBuf,
    ) -> Result<bool, RewindError> {
        self.execute_task_impl(task, engine, Some(work_dir)).await
    }

    /// Shared implementation for execute_task and execute_task_in_dir.
    async fn execute_task_impl<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        task: &TaskView,
        engine: &RewindEngine<B>,
        work_dir_override: Option<PathBuf>,
    ) -> Result<bool, RewindError> {
        let task_id = &task.task_id;
        let task_id_str = task_id.to_string();
        let work_dir = work_dir_override.unwrap_or_else(|| self.work_dir.clone());

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

        let session_id = SessionId::generate();

        info!("Executing: {} ({})", task.title, work_dir.display());

        // EXECUTE: run coder agent
        let prompt_ctx = PromptContext {
            epic_name: self.epic_name.as_deref(),
            project_context: self.project_context.as_deref(),
            template_path: self.prompt_template_path.as_deref(),
            ..Default::default()
        };
        let iteration_start = Instant::now();
        let (tool_calls, agent_output) = self
            .coder
            .execute_task(
                &task.title,
                &task.description,
                &task.acceptance_criteria,
                work_dir,
                self.timeout_secs,
                &prompt_ctx,
            )
            .await?;
        let duration_ms = iteration_start.elapsed().as_millis() as u64;

        // Emit IterationLogged event
        if let Err(e) = engine
            .append_events(vec![RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: task_id.clone(),
                iteration_number: 1,
                agent_output: agent_output.clone(),
                duration_ms,
            }])
            .await
        {
            warn!("Failed to append IterationLogged event: {e}");
        }

        // Record tool calls as events
        for call in &tool_calls {
            if let Err(e) = engine
                .append_events(vec![RewindEvent::AgentToolCall {
                    task_id: task_id.clone(),
                    tool_name: call.tool_name.clone(),
                    args_summary: call.args_summary.clone(),
                    result_summary: call.result_summary.clone(),
                    called_at: Utc::now(),
                }])
                .await
            {
                warn!("Failed to append AgentToolCall event: {e}");
            }
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
                    session_id: session_id.clone(),
                    discretionary_note: None,
                })
                .await?;

            // Mark checked criteria
            for cr in &eval_result.criteria_results {
                if cr.passed {
                    if let Err(e) = engine
                        .append_events(vec![RewindEvent::CriterionChecked {
                            task_id: task_id.clone(),
                            criterion_index: cr.index,
                            checked_at: Utc::now(),
                        }])
                        .await
                    {
                        warn!("Failed to append CriterionChecked event: {e}");
                    }
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
                    session_id: session_id.clone(),
                    reason: reason.clone(),
                    discretionary_note: None,
                })
                .await?;

            if self.use_chronis {
                let _ = ChronisBridge::fail(&task_id_str, &reason);
            }

            Ok(false)
        }
    }

    /// Execute runnable tasks in parallel using git worktrees.
    ///
    /// Returns (completed_count, failed_count).
    #[hotpath::measure]
    pub async fn execute_parallel<
        B: allframe::cqrs::EventStoreBackend<RewindEvent> + Send + Sync + 'static,
    >(
        self: Arc<Self>,
        engine: Arc<RewindEngine<B>>,
        max_concurrent: usize,
    ) -> Result<(usize, usize), RewindError> {
        use crate::application::scheduler::pick_runnable_tasks;

        let worktree_mgr = WorktreeManager::new(self.work_dir.clone());
        let semaphore = Arc::new(Semaphore::new(max_concurrent));
        let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        loop {
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

            let mut handles = Vec::new();

            for task in tasks {
                let permit = semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|e| RewindError::Config(format!("Semaphore error: {e}")))?;

                let task_id_str = task.task_id.to_string();

                // Create worktree
                let worktree_path = match worktree_mgr.create(&task_id_str) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Failed to create worktree for {task_id_str}: {e}");
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        continue;
                    }
                };

                let orchestrator = self.clone();
                let engine = engine.clone();
                let completed = completed.clone();
                let failed = failed.clone();
                let wt_mgr_root = self.work_dir.clone();

                handles.push(tokio::spawn(async move {
                    let _permit = permit;
                    let task_id = task.task_id.to_string();

                    eprintln!("[{task_id}] Starting in worktree...");

                    let result = orchestrator
                        .execute_task_in_dir(&task, &engine, worktree_path)
                        .await;

                    let wt_mgr = WorktreeManager::new(wt_mgr_root);

                    match result {
                        Ok(true) => {
                            eprintln!("[{task_id}] PASSED — merging...");
                            if let Err(e) = wt_mgr.merge_back(&task_id) {
                                eprintln!("[{task_id}] Merge failed: {e}");
                                failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            } else {
                                completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        Ok(false) => {
                            eprintln!("[{task_id}] FAILED");
                            failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        Err(e) => {
                            eprintln!("[{task_id}] ERROR: {e}");
                            failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }

                    wt_mgr.cleanup(&task_id);
                }));
            }

            // Wait for all tasks in this batch
            for handle in handles {
                let _ = handle.await;
            }
        }

        Ok((
            completed.load(std::sync::atomic::Ordering::Relaxed),
            failed.load(std::sync::atomic::Ordering::Relaxed),
        ))
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

    #[tokio::test]
    async fn iteration_logged_event_emitted_with_correct_fields() {
        let engine = RewindEngine::in_memory().await;

        let session_id = SessionId::new("sess-iter-1");
        let task_id = TaskId::new("task-iter-1");

        // Emit an IterationLogged event (as the orchestrator would)
        engine
            .append_events(vec![RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: task_id.clone(),
                iteration_number: 1,
                agent_output: "Implemented the handler".into(),
                duration_ms: 3200,
            }])
            .await
            .unwrap();

        // Emit a second iteration with incremented number
        engine
            .append_events(vec![RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: task_id.clone(),
                iteration_number: 2,
                agent_output: "Fixed test failures".into(),
                duration_ms: 1500,
            }])
            .await
            .unwrap();

        // Read back all events from the store
        let events = engine.event_store.get_all_events().await.unwrap();
        let iteration_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RewindEvent::IterationLogged { .. }))
            .collect();

        assert_eq!(iteration_events.len(), 2, "Should have 2 IterationLogged events");

        match &iteration_events[0] {
            RewindEvent::IterationLogged {
                session_id: sid,
                task_id: tid,
                iteration_number,
                agent_output,
                duration_ms,
            } => {
                assert_eq!(sid.to_string(), "sess-iter-1");
                assert_eq!(tid.to_string(), "task-iter-1");
                assert_eq!(*iteration_number, 1);
                assert_eq!(agent_output, "Implemented the handler");
                assert_eq!(*duration_ms, 3200);
            }
            _ => panic!("Expected IterationLogged"),
        }

        match &iteration_events[1] {
            RewindEvent::IterationLogged {
                iteration_number,
                agent_output,
                duration_ms,
                ..
            } => {
                assert_eq!(*iteration_number, 2);
                assert_eq!(agent_output, "Fixed test failures");
                assert_eq!(*duration_ms, 1500);
            }
            _ => panic!("Expected IterationLogged"),
        }
    }

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
