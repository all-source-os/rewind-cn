use allframe::cqrs::EventTypeName;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{AgentId, EpicId, SessionId, TaskId};

/// Acceptance criterion for a task — a verifiable checkbox the agent must check off.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AcceptanceCriterion {
    pub description: String,
    #[serde(default)]
    pub checked: bool,
}

/// Quality gate tier — determines when the gate runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum GateTier {
    /// Run once when all tasks in the epic complete.
    #[default]
    Epic,
    /// Checked per story where relevant.
    Story,
}

/// Alias used by the gate runner to select which tier to execute.
pub type QualityGateLevel = GateTier;

/// A quality gate — a command that validates the codebase.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct QualityGate {
    pub command: String,
    #[serde(default)]
    pub tier: GateTier,
}

/// Progress note type — categorises the kind of learning captured.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum ProgressNoteType {
    TaskCompleted,
    TaskFailed,
    RetryLearning,
    Discretionary,
}

/// Story type tag — determines which story-level gates apply.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum StoryType {
    Schema,
    Backend,
    UI,
    Integration,
    Infrastructure,
}

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
        #[serde(default)]
        acceptance_criteria: Vec<AcceptanceCriterion>,
        #[serde(default)]
        story_type: Option<StoryType>,
        #[serde(default)]
        depends_on: Vec<TaskId>,
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

    // Criterion events
    CriterionChecked {
        task_id: TaskId,
        criterion_index: usize,
        checked_at: DateTime<Utc>,
    },

    // Epic events
    EpicCreated {
        epic_id: EpicId,
        title: String,
        description: String,
        created_at: DateTime<Utc>,
        #[serde(default)]
        quality_gates: Vec<QualityGate>,
    },
    EpicCompleted {
        epic_id: EpicId,
        completed_at: DateTime<Utc>,
    },

    // Quality gate events
    QualityGateRan {
        epic_id: EpicId,
        command: String,
        passed: bool,
        output: String,
        ran_at: DateTime<Utc>,
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

    // Progress events
    ProgressNoted {
        session_id: SessionId,
        task_id: Option<TaskId>,
        note: String,
        note_type: ProgressNoteType,
        #[serde(default = "Utc::now")]
        noted_at: DateTime<Utc>,
    },

    // Iteration events
    IterationLogged {
        session_id: SessionId,
        task_id: TaskId,
        iteration_number: u32,
        agent_output: String,
        duration_ms: u64,
    },

    // Agent events
    AgentToolCall {
        task_id: TaskId,
        tool_name: String,
        args_summary: String,
        result_summary: String,
        called_at: DateTime<Utc>,
    },
}

impl EventTypeName for RewindEvent {
    fn event_type_name() -> &'static str {
        "rewind.domain.event"
    }
}
impl allframe::cqrs::Event for RewindEvent {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ids::{SessionId, TaskId};

    #[test]
    fn progress_noted_serde_roundtrip() {
        let event = RewindEvent::ProgressNoted {
            session_id: SessionId::new("sess-1"),
            task_id: Some(TaskId::new("task-1")),
            note: "Retry succeeded after increasing timeout".into(),
            note_type: ProgressNoteType::RetryLearning,
            noted_at: Utc::now(),
        };

        let json = serde_json::to_string(&event).expect("serialize");
        let back: RewindEvent = serde_json::from_str(&json).expect("deserialize");

        match back {
            RewindEvent::ProgressNoted {
                session_id,
                task_id,
                note,
                note_type,
                ..
            } => {
                assert_eq!(session_id.to_string(), "sess-1");
                assert_eq!(task_id.unwrap().to_string(), "task-1");
                assert_eq!(note, "Retry succeeded after increasing timeout");
                assert_eq!(note_type, ProgressNoteType::RetryLearning);
            }
            other => panic!("expected ProgressNoted, got {other:?}"),
        }
    }

    #[test]
    fn progress_noted_without_task_id() {
        let event = RewindEvent::ProgressNoted {
            session_id: SessionId::new("sess-2"),
            task_id: None,
            note: "General observation".into(),
            note_type: ProgressNoteType::Discretionary,
            noted_at: Utc::now(),
        };

        let json = serde_json::to_string(&event).expect("serialize");
        let back: RewindEvent = serde_json::from_str(&json).expect("deserialize");

        match back {
            RewindEvent::ProgressNoted { task_id, note_type, .. } => {
                assert!(task_id.is_none());
                assert_eq!(note_type, ProgressNoteType::Discretionary);
            }
            other => panic!("expected ProgressNoted, got {other:?}"),
        }
    }

    #[test]
    fn iteration_logged_serde_roundtrip() {
        let event = RewindEvent::IterationLogged {
            session_id: SessionId::new("sess-1"),
            task_id: TaskId::new("task-1"),
            iteration_number: 3,
            agent_output: "Applied fix to handler.rs".into(),
            duration_ms: 4500,
        };

        let json = serde_json::to_string(&event).expect("serialize");
        let back: RewindEvent = serde_json::from_str(&json).expect("deserialize");

        match back {
            RewindEvent::IterationLogged {
                session_id,
                task_id,
                iteration_number,
                agent_output,
                duration_ms,
            } => {
                assert_eq!(session_id.to_string(), "sess-1");
                assert_eq!(task_id.to_string(), "task-1");
                assert_eq!(iteration_number, 3);
                assert_eq!(agent_output, "Applied fix to handler.rs");
                assert_eq!(duration_ms, 4500);
            }
            other => panic!("expected IterationLogged, got {other:?}"),
        }
    }
}
