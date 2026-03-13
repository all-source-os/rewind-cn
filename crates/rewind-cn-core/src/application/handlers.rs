use chrono::Utc;

use crate::domain::error::RewindError;
use crate::domain::events::{ProgressNoteType, RewindEvent};
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
    let note_text = format!("Task {} completed", cmd.task_id);
    let mut events = vec![RewindEvent::TaskCompleted {
        task_id: cmd.task_id.clone(),
        completed_at: Utc::now(),
    }];
    if let Some(note) = cmd.discretionary_note {
        events.push(RewindEvent::ProgressNoted {
            session_id: cmd.session_id.clone(),
            task_id: Some(cmd.task_id.clone()),
            note: note_text,
            note_type: ProgressNoteType::TaskCompleted,
            noted_at: Utc::now(),
        });
        events.push(RewindEvent::ProgressNoted {
            session_id: cmd.session_id,
            task_id: Some(cmd.task_id),
            note,
            note_type: ProgressNoteType::Discretionary,
            noted_at: Utc::now(),
        });
    } else {
        events.push(RewindEvent::ProgressNoted {
            session_id: cmd.session_id,
            task_id: Some(cmd.task_id),
            note: note_text,
            note_type: ProgressNoteType::TaskCompleted,
            noted_at: Utc::now(),
        });
    }
    Ok(events)
}

pub fn handle_fail_task(cmd: FailTask) -> Result<Vec<RewindEvent>, RewindError> {
    if cmd.reason.trim().is_empty() {
        return Err(RewindError::validation(
            "reason",
            "Failure reason cannot be empty",
        ));
    }
    let note_text = format!("Task {} failed: {}", cmd.task_id, cmd.reason);
    let mut events = vec![RewindEvent::TaskFailed {
        task_id: cmd.task_id.clone(),
        reason: cmd.reason,
        failed_at: Utc::now(),
    }];
    if let Some(note) = cmd.discretionary_note {
        events.push(RewindEvent::ProgressNoted {
            session_id: cmd.session_id.clone(),
            task_id: Some(cmd.task_id.clone()),
            note: note_text,
            note_type: ProgressNoteType::TaskFailed,
            noted_at: Utc::now(),
        });
        events.push(RewindEvent::ProgressNoted {
            session_id: cmd.session_id,
            task_id: Some(cmd.task_id),
            note,
            note_type: ProgressNoteType::Discretionary,
            noted_at: Utc::now(),
        });
    } else {
        events.push(RewindEvent::ProgressNoted {
            session_id: cmd.session_id,
            task_id: Some(cmd.task_id),
            note: note_text,
            note_type: ProgressNoteType::TaskFailed,
            noted_at: Utc::now(),
        });
    }
    Ok(events)
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

    #[test]
    fn complete_task_emits_progress_noted() {
        let events = handle_complete_task(CompleteTask {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            discretionary_note: None,
        })
        .unwrap();

        assert_eq!(events.len(), 2);
        matches!(&events[0], RewindEvent::TaskCompleted { .. });
        match &events[1] {
            RewindEvent::ProgressNoted { note_type, .. } => {
                assert_eq!(note_type, &ProgressNoteType::TaskCompleted);
            }
            _ => panic!("Expected ProgressNoted"),
        }
    }

    #[test]
    fn complete_task_with_discretionary_note() {
        let events = handle_complete_task(CompleteTask {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            discretionary_note: Some("Learned about caching".into()),
        })
        .unwrap();

        assert_eq!(events.len(), 3);
        matches!(&events[0], RewindEvent::TaskCompleted { .. });
        match &events[1] {
            RewindEvent::ProgressNoted { note_type, .. } => {
                assert_eq!(note_type, &ProgressNoteType::TaskCompleted);
            }
            _ => panic!("Expected ProgressNoted TaskCompleted"),
        }
        match &events[2] {
            RewindEvent::ProgressNoted {
                note_type, note, ..
            } => {
                assert_eq!(note_type, &ProgressNoteType::Discretionary);
                assert_eq!(note, "Learned about caching");
            }
            _ => panic!("Expected ProgressNoted Discretionary"),
        }
    }

    #[test]
    fn fail_task_emits_progress_noted() {
        let events = handle_fail_task(FailTask {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            reason: "Tests failed".into(),
            discretionary_note: None,
        })
        .unwrap();

        assert_eq!(events.len(), 2);
        matches!(&events[0], RewindEvent::TaskFailed { .. });
        match &events[1] {
            RewindEvent::ProgressNoted { note_type, .. } => {
                assert_eq!(note_type, &ProgressNoteType::TaskFailed);
            }
            _ => panic!("Expected ProgressNoted"),
        }
    }

    #[test]
    fn fail_task_with_discretionary_note() {
        let events = handle_fail_task(FailTask {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            reason: "Compilation error".into(),
            discretionary_note: Some("Need to check imports".into()),
        })
        .unwrap();

        assert_eq!(events.len(), 3);
        matches!(&events[0], RewindEvent::TaskFailed { .. });
        match &events[1] {
            RewindEvent::ProgressNoted { note_type, .. } => {
                assert_eq!(note_type, &ProgressNoteType::TaskFailed);
            }
            _ => panic!("Expected ProgressNoted TaskFailed"),
        }
        match &events[2] {
            RewindEvent::ProgressNoted {
                note_type, note, ..
            } => {
                assert_eq!(note_type, &ProgressNoteType::Discretionary);
                assert_eq!(note, "Need to check imports");
            }
            _ => panic!("Expected ProgressNoted Discretionary"),
        }
    }

    #[test]
    fn fail_task_rejects_empty_reason() {
        let result = handle_fail_task(FailTask {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            reason: "".into(),
            discretionary_note: None,
        });
        assert!(result.is_err());
    }

    #[test]
    fn fail_task_rejects_whitespace_only_reason() {
        let result = handle_fail_task(FailTask {
            task_id: TaskId::new("t-1"),
            session_id: SessionId::new("s-1"),
            reason: "   ".into(),
            discretionary_note: None,
        });
        assert!(result.is_err());
    }
}
