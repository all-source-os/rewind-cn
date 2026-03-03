use tracing::info;

use crate::application::commands::{AssignTask, CompleteTask, FailTask, StartTask};
use crate::domain::error::RalphError;
use crate::domain::events::RalphEvent;
use crate::domain::ids::{AgentId, TaskId};

use super::engine::RalphEngine;

pub struct AgentWorker {
    pub agent_id: AgentId,
}

impl AgentWorker {
    pub fn new() -> Self {
        Self {
            agent_id: AgentId::generate(),
        }
    }

    /// Execute a single task through the full lifecycle.
    /// Phase 1: immediately completes (no LLM).
    pub async fn execute_task<B: allframe::cqrs::EventStoreBackend<RalphEvent>>(
        &self,
        task_id: TaskId,
        task_title: &str,
        engine: &RalphEngine<B>,
    ) -> Result<(), RalphError> {
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
            Ok(_) => Ok(()),
            Err(e) => {
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
        let engine = RalphEngine::in_memory().await;
        let worker = AgentWorker::new();

        let events = engine
            .create_task(CreateTask {
                title: "Test task".into(),
                description: "".into(),
                epic_id: None,
            })
            .await
            .unwrap();

        let task_id = match &events[0] {
            RalphEvent::TaskCreated { task_id, .. } => task_id.clone(),
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
