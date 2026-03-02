use allframe::cqrs::EventTypeName;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// All domain events in the Ralph system.
///
/// AllFrame's CQRS is generic over a single event type per store,
/// so we use one enum covering Task, Epic, and Session events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RalphEvent {
    // Task events
    TaskCreated {
        task_id: String,
        title: String,
        description: String,
        epic_id: Option<String>,
        created_at: DateTime<Utc>,
    },
    TaskAssigned {
        task_id: String,
        agent_id: String,
        assigned_at: DateTime<Utc>,
    },
    TaskStarted {
        task_id: String,
        started_at: DateTime<Utc>,
    },
    TaskCompleted {
        task_id: String,
        completed_at: DateTime<Utc>,
    },
    TaskFailed {
        task_id: String,
        reason: String,
        failed_at: DateTime<Utc>,
    },
    TaskBlocked {
        task_id: String,
        blocked_by: String,
        blocked_at: DateTime<Utc>,
    },

    // Epic events
    EpicCreated {
        epic_id: String,
        title: String,
        description: String,
        created_at: DateTime<Utc>,
    },
    EpicCompleted {
        epic_id: String,
        completed_at: DateTime<Utc>,
    },

    // Session events
    SessionStarted {
        session_id: String,
        started_at: DateTime<Utc>,
    },
    SessionEnded {
        session_id: String,
        ended_at: DateTime<Utc>,
    },
}

impl EventTypeName for RalphEvent {}
impl allframe::cqrs::Event for RalphEvent {}
