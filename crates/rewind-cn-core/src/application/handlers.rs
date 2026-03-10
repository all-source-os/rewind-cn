use chrono::Utc;

use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;
use crate::domain::ids::{EpicId, SessionId, TaskId};

use super::commands::*;

pub fn handle_create_task(cmd: CreateTask) -> Result<Vec<RewindEvent>, RewindError> {
    if cmd.title.trim().is_empty() {
        return Err(RewindError::validation(
            "title",
            "Task title cannot be empty",
        ));
    }
    Ok(vec![RewindEvent::TaskCreated {
        task_id: TaskId::generate(),
        title: cmd.title,
        description: cmd.description,
        epic_id: cmd.epic_id,
        created_at: Utc::now(),
        acceptance_criteria: cmd.acceptance_criteria,
        story_type: cmd.story_type,
        depends_on: cmd.depends_on,
    }])
}

pub fn handle_assign_task(cmd: AssignTask) -> Result<Vec<RewindEvent>, RewindError> {
    if cmd.agent_id.as_ref().trim().is_empty() {
        return Err(RewindError::validation(
            "agent_id",
            "Agent ID cannot be empty",
        ));
    }
    Ok(vec![RewindEvent::TaskAssigned {
        task_id: cmd.task_id,
        agent_id: cmd.agent_id,
        assigned_at: Utc::now(),
    }])
}

pub fn handle_start_task(cmd: StartTask) -> Result<Vec<RewindEvent>, RewindError> {
    Ok(vec![RewindEvent::TaskStarted {
        task_id: cmd.task_id,
        started_at: Utc::now(),
    }])
}

pub fn handle_complete_task(cmd: CompleteTask) -> Result<Vec<RewindEvent>, RewindError> {
    Ok(vec![RewindEvent::TaskCompleted {
        task_id: cmd.task_id,
        completed_at: Utc::now(),
    }])
}

pub fn handle_fail_task(cmd: FailTask) -> Result<Vec<RewindEvent>, RewindError> {
    Ok(vec![RewindEvent::TaskFailed {
        task_id: cmd.task_id,
        reason: cmd.reason,
        failed_at: Utc::now(),
    }])
}

pub fn handle_create_epic(cmd: CreateEpic) -> Result<Vec<RewindEvent>, RewindError> {
    if cmd.title.trim().is_empty() {
        return Err(RewindError::validation(
            "title",
            "Epic title cannot be empty",
        ));
    }
    Ok(vec![RewindEvent::EpicCreated {
        epic_id: EpicId::generate(),
        title: cmd.title,
        description: cmd.description,
        created_at: Utc::now(),
        quality_gates: cmd.quality_gates,
    }])
}

pub fn handle_complete_epic(cmd: CompleteEpic) -> Result<Vec<RewindEvent>, RewindError> {
    Ok(vec![RewindEvent::EpicCompleted {
        epic_id: cmd.epic_id,
        completed_at: Utc::now(),
    }])
}

pub fn handle_start_session(_cmd: StartSession) -> Result<Vec<RewindEvent>, RewindError> {
    Ok(vec![RewindEvent::SessionStarted {
        session_id: SessionId::generate(),
        started_at: Utc::now(),
    }])
}

pub fn handle_end_session(cmd: EndSession) -> Result<Vec<RewindEvent>, RewindError> {
    Ok(vec![RewindEvent::SessionEnded {
        session_id: cmd.session_id,
        ended_at: Utc::now(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_task_emits_event() {
        let events = handle_create_task(CreateTask {
            title: "Test task".into(),
            description: "A test".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            RewindEvent::TaskCreated { title, .. } => assert_eq!(title, "Test task"),
            _ => panic!("Expected TaskCreated"),
        }
    }

    #[test]
    fn create_task_rejects_empty_title() {
        let result = handle_create_task(CreateTask {
            title: "  ".into(),
            description: "desc".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        assert!(result.is_err());
    }

    #[test]
    fn create_epic_emits_event() {
        let events = handle_create_epic(CreateEpic {
            title: "Sprint 1".into(),
            description: "First sprint".into(),
            quality_gates: vec![],
        })
        .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            RewindEvent::EpicCreated { title, .. } => assert_eq!(title, "Sprint 1"),
            _ => panic!("Expected EpicCreated"),
        }
    }

    #[test]
    fn start_task_emits_event() {
        let events = handle_start_task(StartTask {
            task_id: TaskId::new("t-1"),
        })
        .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            RewindEvent::TaskStarted { task_id, .. } => {
                assert_eq!(task_id, &TaskId::new("t-1"))
            }
            _ => panic!("Expected TaskStarted"),
        }
    }

    #[test]
    fn start_session_emits_event() {
        let events = handle_start_session(StartSession).unwrap();

        assert_eq!(events.len(), 1);
        matches!(&events[0], RewindEvent::SessionStarted { .. });
    }
}
