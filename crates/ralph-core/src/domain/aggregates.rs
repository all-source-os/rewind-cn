use allframe::cqrs::Aggregate;

use super::events::RalphEvent;

/// Task lifecycle states.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TaskStatus {
    #[default]
    Pending,
    Assigned,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

/// Aggregate representing a single task.
#[derive(Debug, Clone, Default)]
pub struct TaskAggregate {
    pub task_id: String,
    pub title: String,
    pub description: String,
    pub epic_id: Option<String>,
    pub status: TaskStatus,
    pub agent_id: Option<String>,
    pub blocked_by: Option<String>,
    pub failure_reason: Option<String>,
}

impl Aggregate for TaskAggregate {
    type Event = RalphEvent;

    fn apply_event(&mut self, event: &RalphEvent) {
        match event {
            RalphEvent::TaskCreated {
                task_id,
                title,
                description,
                epic_id,
                ..
            } => {
                self.task_id = task_id.clone();
                self.title = title.clone();
                self.description = description.clone();
                self.epic_id = epic_id.clone();
                self.status = TaskStatus::Pending;
            }
            RalphEvent::TaskAssigned {
                agent_id, ..
            } => {
                self.agent_id = Some(agent_id.clone());
                self.status = TaskStatus::Assigned;
            }
            RalphEvent::TaskStarted { .. } => {
                self.status = TaskStatus::InProgress;
            }
            RalphEvent::TaskCompleted { .. } => {
                self.status = TaskStatus::Completed;
            }
            RalphEvent::TaskFailed { reason, .. } => {
                self.failure_reason = Some(reason.clone());
                self.status = TaskStatus::Failed;
            }
            RalphEvent::TaskBlocked { blocked_by, .. } => {
                self.blocked_by = Some(blocked_by.clone());
                self.status = TaskStatus::Blocked;
            }
            _ => {} // Ignore non-task events
        }
    }
}

/// Epic lifecycle states.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum EpicStatus {
    #[default]
    Open,
    Completed,
}

/// Aggregate representing an epic (group of tasks).
#[derive(Debug, Clone, Default)]
pub struct EpicAggregate {
    pub epic_id: String,
    pub title: String,
    pub description: String,
    pub status: EpicStatus,
}

impl Aggregate for EpicAggregate {
    type Event = RalphEvent;

    fn apply_event(&mut self, event: &RalphEvent) {
        match event {
            RalphEvent::EpicCreated {
                epic_id,
                title,
                description,
                ..
            } => {
                self.epic_id = epic_id.clone();
                self.title = title.clone();
                self.description = description.clone();
                self.status = EpicStatus::Open;
            }
            RalphEvent::EpicCompleted { .. } => {
                self.status = EpicStatus::Completed;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn task_aggregate_applies_lifecycle() {
        let mut task = TaskAggregate::default();

        task.apply_event(&RalphEvent::TaskCreated {
            task_id: "t-1".into(),
            title: "Fix bug".into(),
            description: "Fix the login bug".into(),
            epic_id: None,
            created_at: Utc::now(),
        });
        assert_eq!(task.task_id, "t-1");
        assert_eq!(task.status, TaskStatus::Pending);

        task.apply_event(&RalphEvent::TaskAssigned {
            task_id: "t-1".into(),
            agent_id: "agent-1".into(),
            assigned_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::Assigned);
        assert_eq!(task.agent_id.as_deref(), Some("agent-1"));

        task.apply_event(&RalphEvent::TaskStarted {
            task_id: "t-1".into(),
            started_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::InProgress);

        task.apply_event(&RalphEvent::TaskCompleted {
            task_id: "t-1".into(),
            completed_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[test]
    fn task_aggregate_applies_failure() {
        let mut task = TaskAggregate::default();

        task.apply_event(&RalphEvent::TaskCreated {
            task_id: "t-2".into(),
            title: "Deploy".into(),
            description: "Deploy to prod".into(),
            epic_id: Some("e-1".into()),
            created_at: Utc::now(),
        });

        task.apply_event(&RalphEvent::TaskFailed {
            task_id: "t-2".into(),
            reason: "Timeout".into(),
            failed_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.failure_reason.as_deref(), Some("Timeout"));
    }

    #[test]
    fn epic_aggregate_applies_lifecycle() {
        let mut epic = EpicAggregate::default();

        epic.apply_event(&RalphEvent::EpicCreated {
            epic_id: "e-1".into(),
            title: "Sprint 1".into(),
            description: "First sprint".into(),
            created_at: Utc::now(),
        });
        assert_eq!(epic.epic_id, "e-1");
        assert_eq!(epic.status, EpicStatus::Open);

        epic.apply_event(&RalphEvent::EpicCompleted {
            epic_id: "e-1".into(),
            completed_at: Utc::now(),
        });
        assert_eq!(epic.status, EpicStatus::Completed);
    }
}
