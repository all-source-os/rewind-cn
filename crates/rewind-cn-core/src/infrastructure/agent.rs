use tracing::{debug, info, warn};

use crate::application::commands::{AssignTask, CompleteTask, FailTask, StartTask};
use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;
use crate::domain::ids::{AgentId, TaskId};

use super::chronis::ChronisBridge;
use super::engine::RewindEngine;

pub struct AgentWorker {
    pub agent_id: AgentId,
    /// When true, sync task lifecycle with chronis (`cn claim`/`cn done`).
    pub use_chronis: bool,
}

impl Default for AgentWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentWorker {
    pub fn new() -> Self {
        let use_chronis = ChronisBridge::is_available();
        if use_chronis {
            debug!("Chronis bridge enabled (cn CLI found)");
        }
        Self {
            agent_id: AgentId::generate(),
            use_chronis,
        }
    }

    /// Create a worker without chronis integration (for tests or offline use).
    pub fn without_chronis() -> Self {
        Self {
            agent_id: AgentId::generate(),
            use_chronis: false,
        }
    }

    /// Execute a single task through the full lifecycle.
    /// Phase 1: immediately completes (no LLM).
    /// When chronis is available, also syncs via `cn claim` / `cn done`.
    pub async fn execute_task<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        task_id: TaskId,
        task_title: &str,
        engine: &RewindEngine<B>,
    ) -> Result<(), RewindError> {
        let task_id_str = task_id.to_string();

        // Claim in chronis (best-effort, non-blocking)
        if self.use_chronis {
            match ChronisBridge::claim(&task_id_str) {
                Ok(ack) => debug!("Chronis claim: {ack}"),
                Err(e) => warn!("Chronis claim failed (continuing): {e}"),
            }
        }

        // Assign
        engine
            .assign_task(AssignTask {
                task_id: task_id.clone(),
                agent_id: self.agent_id.clone(),
            })
            .await?;

        // Start
        engine
            .start_task(StartTask {
                task_id: task_id.clone(),
            })
            .await?;

        info!("Executing: {task_title}");

        // Phase 1: no-op execution, just complete
        match engine
            .complete_task(CompleteTask {
                task_id: task_id.clone(),
            })
            .await
        {
            Ok(_) => {
                // Mark done in chronis (best-effort)
                if self.use_chronis {
                    match ChronisBridge::done(&task_id_str) {
                        Ok(ack) => debug!("Chronis done: {ack}"),
                        Err(e) => warn!("Chronis done failed (continuing): {e}"),
                    }
                }
                Ok(())
            }
            Err(e) => {
                // Report failure to chronis
                if self.use_chronis {
                    let _ = ChronisBridge::fail(&task_id_str, &e.to_string());
                }
                let _ = engine
                    .fail_task(FailTask {
                        task_id,
                        reason: e.to_string(),
                    })
                    .await;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::commands::CreateTask;
    use crate::domain::model::TaskStatus;

    #[tokio::test]
    async fn agent_executes_task_to_completion() {
        let engine = RewindEngine::in_memory().await;
        let worker = AgentWorker::without_chronis();

        let events = engine
            .create_task(CreateTask {
                title: "Test task".into(),
                description: "".into(),
                epic_id: None,
            })
            .await
            .unwrap();

        let task_id = match &events[0] {
            RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
            _ => panic!("Expected TaskCreated"),
        };

        worker
            .execute_task(task_id.clone(), "Test task", &engine)
            .await
            .unwrap();

        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        let task = backlog.tasks.get(task_id.as_ref()).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
    }
}
