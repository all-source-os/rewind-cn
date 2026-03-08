use allframe::cqrs::EventTypeName;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::{AgentId, EpicId, SessionId, TaskId};

/// All domain events in the Rewind system.
///
/// AllFrame's CQRS is generic over a single event type per store,
/// so we use one enum covering Task, Epic, and Session events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RewindEvent {
    // Task events
    TaskCreated {
        task_id: TaskId,
        title: String,
        description: String,
        epic_id: Option<EpicId>,
        created_at: DateTime<Utc>,
    },
    TaskAssigned {
        task_id: TaskId,
        agent_id: AgentId,
        assigned_at: DateTime<Utc>,
    },
    TaskStarted {
        task_id: TaskId,
        started_at: DateTime<Utc>,
    },
    TaskCompleted {
        task_id: TaskId,
        completed_at: DateTime<Utc>,
    },
    TaskFailed {
        task_id: TaskId,
        reason: String,
        failed_at: DateTime<Utc>,
    },
    TaskBlocked {
        task_id: TaskId,
        blocked_by: TaskId,
        blocked_at: DateTime<Utc>,
    },

    // Epic events
    EpicCreated {
        epic_id: EpicId,
        title: String,
        description: String,
        created_at: DateTime<Utc>,
    },
    EpicCompleted {
        epic_id: EpicId,
        completed_at: DateTime<Utc>,
    },

    // Session events
    SessionStarted {
        session_id: SessionId,
        started_at: DateTime<Utc>,
    },
    SessionEnded {
        session_id: SessionId,
        ended_at: DateTime<Utc>,
    },
}

impl EventTypeName for RewindEvent {
    fn event_type_name() -> &'static str {
        "rewind.domain.event"
    }
}
impl allframe::cqrs::Event for RewindEvent {}
