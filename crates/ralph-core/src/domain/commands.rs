use allframe::cqrs::{Command, CommandError, CommandHandler, CommandResult, ValidationError};
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use super::events::RalphEvent;

// --- Command structs ---

pub struct CreateTask {
    pub title: String,
    pub description: String,
    pub epic_id: Option<String>,
}
impl Command for CreateTask {}

pub struct AssignTask {
    pub task_id: String,
    pub agent_id: String,
}
impl Command for AssignTask {}

pub struct CompleteTask {
    pub task_id: String,
}
impl Command for CompleteTask {}

pub struct FailTask {
    pub task_id: String,
    pub reason: String,
}
impl Command for FailTask {}

pub struct CreateEpic {
    pub title: String,
    pub description: String,
}
impl Command for CreateEpic {}

pub struct CompleteEpic {
    pub epic_id: String,
}
impl Command for CompleteEpic {}

// --- Handlers ---

pub struct CreateTaskHandler;

#[async_trait]
impl CommandHandler<CreateTask, RalphEvent> for CreateTaskHandler {
    async fn handle(&self, cmd: CreateTask) -> CommandResult<RalphEvent> {
        if cmd.title.trim().is_empty() {
            return Err(CommandError::Validation(vec![
                ValidationError::new("title", "Task title cannot be empty"),
            ]));
        }
        Ok(vec![RalphEvent::TaskCreated {
            task_id: Uuid::new_v4().to_string(),
            title: cmd.title,
            description: cmd.description,
            epic_id: cmd.epic_id,
            created_at: Utc::now(),
        }])
    }
}

pub struct AssignTaskHandler;

#[async_trait]
impl CommandHandler<AssignTask, RalphEvent> for AssignTaskHandler {
    async fn handle(&self, cmd: AssignTask) -> CommandResult<RalphEvent> {
        if cmd.agent_id.trim().is_empty() {
            return Err(CommandError::Validation(vec![
                ValidationError::new("agent_id", "Agent ID cannot be empty"),
            ]));
        }
        Ok(vec![RalphEvent::TaskAssigned {
            task_id: cmd.task_id,
            agent_id: cmd.agent_id,
            assigned_at: Utc::now(),
        }])
    }
}

pub struct CompleteTaskHandler;

#[async_trait]
impl CommandHandler<CompleteTask, RalphEvent> for CompleteTaskHandler {
    async fn handle(&self, cmd: CompleteTask) -> CommandResult<RalphEvent> {
        Ok(vec![RalphEvent::TaskCompleted {
            task_id: cmd.task_id,
            completed_at: Utc::now(),
        }])
    }
}

pub struct FailTaskHandler;

#[async_trait]
impl CommandHandler<FailTask, RalphEvent> for FailTaskHandler {
    async fn handle(&self, cmd: FailTask) -> CommandResult<RalphEvent> {
        Ok(vec![RalphEvent::TaskFailed {
            task_id: cmd.task_id,
            reason: cmd.reason,
            failed_at: Utc::now(),
        }])
    }
}

pub struct CreateEpicHandler;

#[async_trait]
impl CommandHandler<CreateEpic, RalphEvent> for CreateEpicHandler {
    async fn handle(&self, cmd: CreateEpic) -> CommandResult<RalphEvent> {
        if cmd.title.trim().is_empty() {
            return Err(CommandError::Validation(vec![
                ValidationError::new("title", "Epic title cannot be empty"),
            ]));
        }
        Ok(vec![RalphEvent::EpicCreated {
            epic_id: Uuid::new_v4().to_string(),
            title: cmd.title,
            description: cmd.description,
            created_at: Utc::now(),
        }])
    }
}

pub struct CompleteEpicHandler;

#[async_trait]
impl CommandHandler<CompleteEpic, RalphEvent> for CompleteEpicHandler {
    async fn handle(&self, cmd: CompleteEpic) -> CommandResult<RalphEvent> {
        Ok(vec![RalphEvent::EpicCompleted {
            epic_id: cmd.epic_id,
            completed_at: Utc::now(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_task_emits_event() {
        let handler = CreateTaskHandler;
        let events = handler
            .handle(CreateTask {
                title: "Test task".into(),
                description: "A test".into(),
                epic_id: None,
            })
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            RalphEvent::TaskCreated { title, .. } => assert_eq!(title, "Test task"),
            _ => panic!("Expected TaskCreated"),
        }
    }

    #[tokio::test]
    async fn create_task_rejects_empty_title() {
        let handler = CreateTaskHandler;
        let result = handler
            .handle(CreateTask {
                title: "  ".into(),
                description: "desc".into(),
                epic_id: None,
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn create_epic_emits_event() {
        let handler = CreateEpicHandler;
        let events = handler
            .handle(CreateEpic {
                title: "Sprint 1".into(),
                description: "First sprint".into(),
            })
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            RalphEvent::EpicCreated { title, .. } => assert_eq!(title, "Sprint 1"),
            _ => panic!("Expected EpicCreated"),
        }
    }
}
