use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use crate::application::commands::{AssignTask, CompleteTask, FailTask, StartTask};
use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;
use crate::domain::ids::{AgentId, SessionId, TaskId};
use crate::domain::model::TaskView;
use crate::infrastructure::chronis::ChronisBridge;
use crate::infrastructure::coder::{CoderAgent, PromptContext, TaskExecutor, ToolCallRecord};
use crate::infrastructure::engine::RewindEngine;
use crate::infrastructure::evaluator::EvaluatorAgent;
use crate::infrastructure::llm::{AgentConfig, ProviderClient};
use crate::infrastructure::worktree::WorktreeManager;

/// Orchestrator runs the SELECT → PROMPT → EXECUTE → EVALUATE loop.
pub struct Orchestrator {
    coder: Box<dyn TaskExecutor>,
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
            coder: Box::new(CoderAgent::new(coder_client, config.clone())),
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
        coder: Box<dyn TaskExecutor>,
        evaluator_client: ProviderClient,
        config: AgentConfig,
        work_dir: PathBuf,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Self {
        Self {
            coder,
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

        // Termination guarantee:
        // - `retry_counts` is a local HashMap that persists across loop iterations,
        //   so per-task retry count is never lost even when a task is re-queued.
        // - When retries < max_retries: TaskRetried resets the task to Pending,
        //   making it runnable again — but the counter in retry_counts still
        //   increments, bounding total attempts to max_retries per task.
        // - When retries >= max_retries: task stays Failed, increments `failed`,
        //   and pick_runnable_tasks will never select it again.
        // - If engine.retry_task() itself fails: task stays Failed (not re-queued),
        //   so it won't be picked again. We increment `failed` and move on.
        // - The loop breaks when pick_runnable_tasks returns empty, which must
        //   eventually happen because each task either completes or exhausts retries.
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

            let batch_size = tasks.len();
            for (i, task) in tasks.iter().enumerate() {
                info!(
                    task = %task.title,
                    batch_progress = format!("{}/{}", i + 1, batch_size),
                    completed,
                    failed,
                    "Executing task"
                );

                match self.execute_task(task, engine).await {
                    Ok(true) => {
                        info!(task = %task.title, "Task completed successfully");
                        completed += 1;
                    }
                    Ok(false) => {
                        let retries = retry_counts.entry(task.task_id.to_string()).or_insert(0);
                        *retries += 1;

                        if *retries < self.max_retries {
                            warn!(
                                task = %task.title,
                                retry = *retries,
                                max_retries = self.max_retries,
                                "Task failed, retrying"
                            );
                            // Re-queue: emit TaskRetried to reset status to Pending
                            if let Err(e) = engine
                                .retry_task(crate::application::commands::RetryTask {
                                    task_id: task.task_id.clone(),
                                    retry_number: *retries,
                                })
                                .await
                            {
                                warn!(task = %task.title, error = %e, "Failed to re-queue task for retry");
                                failed += 1;
                            }
                        } else {
                            error!(
                                task = %task.title,
                                max_retries = self.max_retries,
                                "Task failed, max retries reached"
                            );
                            failed += 1;
                        }
                    }
                    Err(e) => {
                        error!(task = %task.title, error = %e, "Task execution error");
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

                    // RAII guard ensures worktree cleanup even on panic
                    let _guard = WorktreeGuard {
                        work_dir: wt_mgr_root.clone(),
                        task_id: task_id.clone(),
                    };

                    info!(task_id = %task_id, "Starting task in worktree");

                    let result = orchestrator
                        .execute_task_in_dir(&task, &engine, worktree_path)
                        .await;

                    let wt_mgr = WorktreeManager::new(wt_mgr_root);

                    match result {
                        Ok(true) => {
                            info!(task_id = %task_id, "Task passed, merging worktree");
                            if let Err(e) = wt_mgr.merge_back(&task_id) {
                                error!(task_id = %task_id, error = %e, "Worktree merge failed");
                                failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            } else {
                                completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        Ok(false) => {
                            warn!(task_id = %task_id, "Task failed evaluation");
                            failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        Err(e) => {
                            error!(task_id = %task_id, error = %e, "Task execution error");
                            failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }));
            }

            // Wait for all tasks in this batch, detect panics
            for handle in handles {
                if let Err(e) = handle.await {
                    warn!("Task panicked: {e}");
                    failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
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

/// RAII guard that cleans up a worktree when dropped, even on panic.
struct WorktreeGuard {
    work_dir: PathBuf,
    task_id: String,
}

impl Drop for WorktreeGuard {
    fn drop(&mut self) {
        let wt_mgr = WorktreeManager::new(self.work_dir.clone());
        wt_mgr.cleanup(&self.task_id);
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

        assert_eq!(
            iteration_events.len(),
            2,
            "Should have 2 IterationLogged events"
        );

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

    /// Mock evaluator client that always returns the given string.
    fn mock_eval_client(response: &str) -> ProviderClient {
        let resp = response.to_string();
        ProviderClient::Mock(std::sync::Arc::new(move |_, _, _, _| resp.clone()))
    }

    /// Mock evaluator client with sequenced responses.
    fn mock_eval_client_sequenced(responses: Vec<String>) -> ProviderClient {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        ProviderClient::Mock(std::sync::Arc::new(move |_, _, _, _| {
            let idx = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            responses
                .get(idx)
                .cloned()
                .unwrap_or_else(|| responses.last().cloned().unwrap_or_default())
        }))
    }

    /// JSON for an evaluator response that passes all criteria.
    fn eval_pass_json() -> String {
        r#"{"passed":true,"criteria_results":[{"index":0,"passed":true,"reason":"Done"}],"summary":"All good"}"#.into()
    }

    /// JSON for an evaluator response that fails.
    fn eval_fail_json() -> String {
        r#"{"passed":false,"criteria_results":[{"index":0,"passed":false,"reason":"Not done"}],"summary":"Failed"}"#.into()
    }

    /// Mock TaskExecutor that returns a fixed response string (no tool calls).
    struct MockTaskExecutor {
        response: String,
    }

    impl MockTaskExecutor {
        fn new(response: &str) -> Box<dyn TaskExecutor> {
            Box::new(Self {
                response: response.to_string(),
            })
        }
    }

    #[async_trait::async_trait]
    impl TaskExecutor for MockTaskExecutor {
        async fn execute_task(
            &self,
            _task_title: &str,
            _task_description: &str,
            _acceptance_criteria: &[crate::domain::events::AcceptanceCriterion],
            _work_dir: PathBuf,
            _timeout_secs: u64,
            _prompt_ctx: &PromptContext<'_>,
        ) -> Result<(Vec<ToolCallRecord>, String), crate::domain::error::RewindError> {
            Ok((vec![], self.response.clone()))
        }
    }

    #[tokio::test]
    async fn execute_runnable_completes_single_task() {
        use crate::application::commands::CreateTask;
        use crate::domain::events::AcceptanceCriterion;
        use crate::domain::model::TaskStatus;

        let engine = RewindEngine::in_memory().await;

        // Create a task
        engine
            .create_task(CreateTask {
                title: "Test task".into(),
                description: "A test task".into(),
                epic_id: None,
                acceptance_criteria: vec![AcceptanceCriterion {
                    description: "It works".into(),
                    checked: false,
                }],
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        let config = AgentConfig::default();
        let work_dir = std::env::temp_dir().join("rewind-orch-test-1");
        std::fs::create_dir_all(&work_dir).ok();

        let orchestrator = Orchestrator::without_chronis(
            MockTaskExecutor::new("Task completed successfully"),
            mock_eval_client(&eval_pass_json()),
            config,
            work_dir.clone(),
            30,
            3,
        );

        let (completed, failed) = orchestrator.execute_runnable(&engine, 1).await.unwrap();

        assert_eq!(completed, 1, "Expected 1 completed task");
        assert_eq!(failed, 0, "Expected 0 failed tasks");

        // Verify task reached Completed status
        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        let task = backlog.tasks.values().next().unwrap();
        assert_eq!(task.status, TaskStatus::Completed);

        std::fs::remove_dir_all(&work_dir).ok();
    }

    #[tokio::test]
    async fn execute_runnable_handles_multiple_tasks() {
        use crate::application::commands::CreateTask;
        use crate::domain::events::AcceptanceCriterion;

        let engine = RewindEngine::in_memory().await;

        for i in 0..3 {
            engine
                .create_task(CreateTask {
                    title: format!("Task {i}"),
                    description: format!("Description {i}"),
                    epic_id: None,
                    acceptance_criteria: vec![AcceptanceCriterion {
                        description: "Criterion".into(),
                        checked: false,
                    }],
                    story_type: None,
                    depends_on: vec![],
                })
                .await
                .unwrap();
        }

        let config = AgentConfig::default();
        let work_dir = std::env::temp_dir().join("rewind-orch-test-multi");
        std::fs::create_dir_all(&work_dir).ok();

        let orchestrator = Orchestrator::without_chronis(
            MockTaskExecutor::new("Done"),
            mock_eval_client(&eval_pass_json()),
            config,
            work_dir.clone(),
            30,
            3,
        );

        let (completed, failed) = orchestrator.execute_runnable(&engine, 5).await.unwrap();

        assert_eq!(completed, 3, "Expected 3 completed tasks");
        assert_eq!(failed, 0, "Expected 0 failed tasks");

        std::fs::remove_dir_all(&work_dir).ok();
    }

    #[tokio::test]
    async fn execute_runnable_fails_task_after_max_retries() {
        use crate::application::commands::CreateTask;
        use crate::domain::events::AcceptanceCriterion;
        use crate::domain::model::TaskStatus;

        let engine = RewindEngine::in_memory().await;

        engine
            .create_task(CreateTask {
                title: "Failing task".into(),
                description: "Will always fail".into(),
                epic_id: None,
                acceptance_criteria: vec![AcceptanceCriterion {
                    description: "Impossible".into(),
                    checked: false,
                }],
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        let config = AgentConfig::default();
        let work_dir = std::env::temp_dir().join("rewind-orch-test-fail");
        std::fs::create_dir_all(&work_dir).ok();

        let max_retries = 2;
        let orchestrator = Orchestrator::without_chronis(
            MockTaskExecutor::new("Attempted the task"),
            mock_eval_client(&eval_fail_json()),
            config,
            work_dir.clone(),
            30,
            max_retries,
        );

        let (completed, failed) = orchestrator.execute_runnable(&engine, 1).await.unwrap();

        assert_eq!(completed, 0, "Expected 0 completed tasks");
        assert_eq!(failed, 1, "Expected 1 failed task after max retries");

        // Verify task reached Failed status
        engine.rebuild_projections().await.unwrap();
        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        let task = backlog.tasks.values().next().unwrap();
        assert_eq!(task.status, TaskStatus::Failed);

        std::fs::remove_dir_all(&work_dir).ok();
    }

    #[tokio::test]
    async fn execute_runnable_retry_then_succeed() {
        use crate::application::commands::CreateTask;
        use crate::domain::events::AcceptanceCriterion;
        use crate::domain::model::TaskStatus;

        let engine = RewindEngine::in_memory().await;

        engine
            .create_task(CreateTask {
                title: "Retry task".into(),
                description: "Fails first, passes second".into(),
                epic_id: None,
                acceptance_criteria: vec![AcceptanceCriterion {
                    description: "Eventually works".into(),
                    checked: false,
                }],
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        let config = AgentConfig::default();
        let work_dir = std::env::temp_dir().join("rewind-orch-test-retry");
        std::fs::create_dir_all(&work_dir).ok();

        // Evaluator: first call fails, second call passes
        let max_retries = 3;
        let orchestrator = Orchestrator::without_chronis(
            MockTaskExecutor::new("Working on it"),
            mock_eval_client_sequenced(vec![eval_fail_json(), eval_pass_json()]),
            config,
            work_dir.clone(),
            30,
            max_retries,
        );

        let (completed, failed) = orchestrator.execute_runnable(&engine, 1).await.unwrap();

        assert_eq!(completed, 1, "Expected task to eventually complete");
        assert_eq!(failed, 0, "Expected 0 failed tasks");

        // Verify task reached Completed status
        engine.rebuild_projections().await.unwrap();
        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        let task = backlog.tasks.values().next().unwrap();
        assert_eq!(task.status, TaskStatus::Completed);

        // Verify retry event was emitted
        let events = engine.event_store.get_all_events().await.unwrap();
        let retry_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RewindEvent::TaskRetried { .. }))
            .collect();
        assert_eq!(retry_events.len(), 1, "Expected exactly 1 retry event");

        std::fs::remove_dir_all(&work_dir).ok();
    }

    #[tokio::test]
    async fn execute_runnable_respects_dependency_order() {
        use crate::application::commands::CreateTask;
        use crate::domain::events::AcceptanceCriterion;
        use crate::domain::model::TaskStatus;

        let engine = RewindEngine::in_memory().await;

        // Create task A (no deps)
        let events_a = engine
            .create_task(CreateTask {
                title: "Task A".into(),
                description: "First task".into(),
                epic_id: None,
                acceptance_criteria: vec![AcceptanceCriterion {
                    description: "A works".into(),
                    checked: false,
                }],
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        let task_a_id = match &events_a[0] {
            RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
            _ => panic!("Expected TaskCreated"),
        };

        // Create task B that depends on A
        engine
            .create_task(CreateTask {
                title: "Task B".into(),
                description: "Depends on A".into(),
                epic_id: None,
                acceptance_criteria: vec![AcceptanceCriterion {
                    description: "B works".into(),
                    checked: false,
                }],
                story_type: None,
                depends_on: vec![task_a_id],
            })
            .await
            .unwrap();

        let config = AgentConfig::default();
        let work_dir = std::env::temp_dir().join("rewind-orch-test-deps");
        std::fs::create_dir_all(&work_dir).ok();

        let orchestrator = Orchestrator::without_chronis(
            MockTaskExecutor::new("Done"),
            mock_eval_client(&eval_pass_json()),
            config,
            work_dir.clone(),
            30,
            3,
        );

        // With max_concurrent=1, only Task A should run first (B is blocked)
        let (completed, failed) = orchestrator.execute_runnable(&engine, 1).await.unwrap();

        assert_eq!(completed, 2, "Both tasks should eventually complete");
        assert_eq!(failed, 0, "No tasks should fail");

        // Verify both tasks are completed
        engine.rebuild_projections().await.unwrap();
        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        for task in backlog.tasks.values() {
            assert_eq!(
                task.status,
                TaskStatus::Completed,
                "Task '{}' should be completed",
                task.title
            );
        }

        std::fs::remove_dir_all(&work_dir).ok();
    }

    #[tokio::test]
    async fn execute_runnable_empty_backlog_returns_zeros() {
        let engine = RewindEngine::in_memory().await;

        let config = AgentConfig::default();
        let work_dir = std::env::temp_dir().join("rewind-orch-test-empty");
        std::fs::create_dir_all(&work_dir).ok();

        let orchestrator = Orchestrator::without_chronis(
            MockTaskExecutor::new("unused"),
            mock_eval_client("unused"),
            config,
            work_dir.clone(),
            30,
            3,
        );

        let (completed, failed) = orchestrator.execute_runnable(&engine, 1).await.unwrap();

        assert_eq!(completed, 0);
        assert_eq!(failed, 0);

        std::fs::remove_dir_all(&work_dir).ok();
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
