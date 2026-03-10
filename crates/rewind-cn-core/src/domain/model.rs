use std::collections::{HashMap, HashSet};
use std::fmt;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::events::{AcceptanceCriterion, QualityGate, RewindEvent, StoryType};
use super::ids::{AgentId, EpicId, TaskId};

// --- Aggregates ---

/// Task lifecycle states.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
pub enum TaskStatus {
    #[default]
    Pending,
    Assigned,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::Assigned => write!(f, "assigned"),
            TaskStatus::InProgress => write!(f, "in-progress"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Blocked => write!(f, "blocked"),
        }
    }
}

/// Aggregate representing a single task.
#[derive(Debug, Clone, Default)]
pub struct TaskAggregate {
    pub task_id: TaskId,
    pub title: String,
    pub description: String,
    pub epic_id: Option<EpicId>,
    pub status: TaskStatus,
    pub agent_id: Option<AgentId>,
    pub blocked_by: Option<TaskId>,
    pub failure_reason: Option<String>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub story_type: Option<StoryType>,
    pub depends_on: Vec<TaskId>,
}

impl TaskAggregate {
    pub fn apply_event(&mut self, event: &RewindEvent) {
        match event {
            RewindEvent::TaskCreated {
                task_id,
                title,
                description,
                epic_id,
                acceptance_criteria,
                story_type,
                depends_on,
                ..
            } => {
                self.task_id = task_id.clone();
                self.title = title.clone();
                self.description = description.clone();
                self.epic_id = epic_id.clone();
                self.acceptance_criteria = acceptance_criteria.clone();
                self.story_type = story_type.clone();
                self.depends_on = depends_on.clone();
                self.status = TaskStatus::Pending;
            }
            RewindEvent::TaskAssigned { agent_id, .. } => {
                self.agent_id = Some(agent_id.clone());
                self.status = TaskStatus::Assigned;
            }
            RewindEvent::TaskStarted { .. } => {
                self.status = TaskStatus::InProgress;
            }
            RewindEvent::TaskCompleted { .. } => {
                self.status = TaskStatus::Completed;
            }
            RewindEvent::TaskFailed { reason, .. } => {
                self.failure_reason = Some(reason.clone());
                self.status = TaskStatus::Failed;
            }
            RewindEvent::TaskBlocked { blocked_by, .. } => {
                self.blocked_by = Some(blocked_by.clone());
                self.status = TaskStatus::Blocked;
            }
            _ => {} // Ignore non-task events
        }
    }
}

/// Epic lifecycle states.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub enum EpicStatus {
    #[default]
    Open,
    Completed,
}

impl fmt::Display for EpicStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpicStatus::Open => write!(f, "open"),
            EpicStatus::Completed => write!(f, "completed"),
        }
    }
}

/// Aggregate representing an epic (group of tasks).
#[derive(Debug, Clone, Default)]
pub struct EpicAggregate {
    pub epic_id: EpicId,
    pub title: String,
    pub description: String,
    pub status: EpicStatus,
    pub quality_gates: Vec<QualityGate>,
}

impl EpicAggregate {
    pub fn apply_event(&mut self, event: &RewindEvent) {
        match event {
            RewindEvent::EpicCreated {
                epic_id,
                title,
                description,
                quality_gates,
                ..
            } => {
                self.epic_id = epic_id.clone();
                self.title = title.clone();
                self.description = description.clone();
                self.quality_gates = quality_gates.clone();
                self.status = EpicStatus::Open;
            }
            RewindEvent::EpicCompleted { .. } => {
                self.status = EpicStatus::Completed;
            }
            _ => {}
        }
    }
}

// --- Read Models / Projections ---

/// Read model for a single task in the backlog.
#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub task_id: TaskId,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub epic_id: Option<EpicId>,
    pub agent_id: Option<AgentId>,
    pub created_at: DateTime<Utc>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub story_type: Option<StoryType>,
    pub depends_on: Vec<TaskId>,
}

/// Projection that maintains the full backlog as a HashMap of task views.
#[derive(Debug, Default)]
pub struct BacklogProjection {
    pub tasks: HashMap<String, TaskView>,
    /// Set of completed task IDs for fast dependency lookups.
    completed_tasks: HashSet<String>,
}

impl BacklogProjection {
    pub fn pending_tasks(&self) -> Vec<&TaskView> {
        self.tasks
            .values()
            .filter(|t| t.status == TaskStatus::Pending)
            .collect()
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Returns true if any of the task's dependencies are not yet completed.
    pub fn is_blocked(&self, task_id: &str) -> bool {
        if let Some(task) = self.tasks.get(task_id) {
            task.depends_on
                .iter()
                .any(|dep| !self.completed_tasks.contains(dep.as_ref()))
        } else {
            false
        }
    }

    /// Returns the number of checked criteria for a task.
    pub fn criteria_checked_count(&self, task_id: &str) -> (usize, usize) {
        if let Some(task) = self.tasks.get(task_id) {
            let checked = task
                .acceptance_criteria
                .iter()
                .filter(|c| c.checked)
                .count();
            (checked, task.acceptance_criteria.len())
        } else {
            (0, 0)
        }
    }

    pub fn apply_event(&mut self, event: &RewindEvent) {
        match event {
            RewindEvent::TaskCreated {
                task_id,
                title,
                description,
                epic_id,
                created_at,
                acceptance_criteria,
                story_type,
                depends_on,
            } => {
                self.tasks.insert(
                    task_id.to_string(),
                    TaskView {
                        task_id: task_id.clone(),
                        title: title.clone(),
                        description: description.clone(),
                        status: TaskStatus::Pending,
                        epic_id: epic_id.clone(),
                        agent_id: None,
                        created_at: *created_at,
                        acceptance_criteria: acceptance_criteria.clone(),
                        story_type: story_type.clone(),
                        depends_on: depends_on.clone(),
                    },
                );
            }
            RewindEvent::TaskAssigned {
                task_id, agent_id, ..
            } => {
                if let Some(task) = self.tasks.get_mut(task_id.as_ref()) {
                    task.status = TaskStatus::Assigned;
                    task.agent_id = Some(agent_id.clone());
                }
            }
            RewindEvent::TaskStarted { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id.as_ref()) {
                    task.status = TaskStatus::InProgress;
                }
            }
            RewindEvent::TaskCompleted { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id.as_ref()) {
                    task.status = TaskStatus::Completed;
                }
                self.completed_tasks.insert(task_id.to_string());
            }
            RewindEvent::TaskFailed { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id.as_ref()) {
                    task.status = TaskStatus::Failed;
                }
            }
            RewindEvent::TaskBlocked { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id.as_ref()) {
                    task.status = TaskStatus::Blocked;
                }
            }
            RewindEvent::CriterionChecked {
                task_id,
                criterion_index,
                ..
            } => {
                if let Some(task) = self.tasks.get_mut(task_id.as_ref()) {
                    if let Some(criterion) = task.acceptance_criteria.get_mut(*criterion_index) {
                        criterion.checked = true;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Tracks progress of epics (how many tasks completed vs total).
#[derive(Debug, Default)]
pub struct EpicProgressProjection {
    pub epics: HashMap<String, EpicProgress>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EpicProgress {
    pub epic_id: EpicId,
    pub title: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub is_completed: bool,
    pub quality_gates: Vec<QualityGate>,
}

impl EpicProgressProjection {
    pub fn progress_pct(&self, epic_id: &str) -> Option<f64> {
        self.epics.get(epic_id).map(|e| {
            if e.total_tasks == 0 {
                0.0
            } else {
                e.completed_tasks as f64 / e.total_tasks as f64 * 100.0
            }
        })
    }

    pub fn apply_event(&mut self, event: &RewindEvent) {
        match event {
            RewindEvent::EpicCreated {
                epic_id,
                title,
                quality_gates,
                ..
            } => {
                self.epics.insert(
                    epic_id.to_string(),
                    EpicProgress {
                        epic_id: epic_id.clone(),
                        title: title.clone(),
                        quality_gates: quality_gates.clone(),
                        ..Default::default()
                    },
                );
            }
            RewindEvent::EpicCompleted { epic_id, .. } => {
                if let Some(epic) = self.epics.get_mut(epic_id.as_ref()) {
                    epic.is_completed = true;
                }
            }
            RewindEvent::TaskCreated {
                epic_id: Some(eid), ..
            } => {
                if let Some(epic) = self.epics.get_mut(eid.as_ref()) {
                    epic.total_tasks += 1;
                }
            }
            RewindEvent::TaskCreated { epic_id: None, .. } => {}
            RewindEvent::TaskCompleted { task_id, .. } => {
                // Known simplification: we'd need cross-reference to find the epic.
                let _ = task_id;
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

        task.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Fix bug".into(),
            description: "Fix the login bug".into(),
            epic_id: None,
            created_at: Utc::now(),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });
        assert_eq!(task.task_id, TaskId::new("t-1"));
        assert_eq!(task.status, TaskStatus::Pending);

        task.apply_event(&RewindEvent::TaskAssigned {
            task_id: TaskId::new("t-1"),
            agent_id: AgentId::new("agent-1"),
            assigned_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::Assigned);
        assert_eq!(task.agent_id.as_ref().map(|a| a.as_ref()), Some("agent-1"));

        task.apply_event(&RewindEvent::TaskStarted {
            task_id: TaskId::new("t-1"),
            started_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::InProgress);

        task.apply_event(&RewindEvent::TaskCompleted {
            task_id: TaskId::new("t-1"),
            completed_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[test]
    fn task_aggregate_applies_failure() {
        let mut task = TaskAggregate::default();

        task.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-2"),
            title: "Deploy".into(),
            description: "Deploy to prod".into(),
            epic_id: Some(EpicId::new("e-1")),
            created_at: Utc::now(),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        task.apply_event(&RewindEvent::TaskFailed {
            task_id: TaskId::new("t-2"),
            reason: "Timeout".into(),
            failed_at: Utc::now(),
        });
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.failure_reason.as_deref(), Some("Timeout"));
    }

    #[test]
    fn epic_aggregate_applies_lifecycle() {
        let mut epic = EpicAggregate::default();

        epic.apply_event(&RewindEvent::EpicCreated {
            epic_id: EpicId::new("e-1"),
            title: "Sprint 1".into(),
            description: "First sprint".into(),
            created_at: Utc::now(),
            quality_gates: vec![],
        });
        assert_eq!(epic.epic_id, EpicId::new("e-1"));
        assert_eq!(epic.status, EpicStatus::Open);

        epic.apply_event(&RewindEvent::EpicCompleted {
            epic_id: EpicId::new("e-1"),
            completed_at: Utc::now(),
        });
        assert_eq!(epic.status, EpicStatus::Completed);
    }

    #[test]
    fn backlog_projection_tracks_tasks() {
        let mut proj = BacklogProjection::default();

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Task 1".into(),
            description: "Desc".into(),
            epic_id: None,
            created_at: Utc::now(),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        assert_eq!(proj.task_count(), 1);
        assert_eq!(proj.pending_tasks().len(), 1);

        proj.apply_event(&RewindEvent::TaskCompleted {
            task_id: TaskId::new("t-1"),
            completed_at: Utc::now(),
        });

        assert_eq!(proj.pending_tasks().len(), 0);
        assert_eq!(proj.tasks.get("t-1").unwrap().status, TaskStatus::Completed);
    }

    #[test]
    fn epic_progress_tracks_tasks() {
        let mut proj = EpicProgressProjection::default();

        proj.apply_event(&RewindEvent::EpicCreated {
            epic_id: EpicId::new("e-1"),
            title: "Sprint 1".into(),
            description: "First sprint".into(),
            created_at: Utc::now(),
            quality_gates: vec![],
        });

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Task 1".into(),
            description: "Desc".into(),
            epic_id: Some(EpicId::new("e-1")),
            created_at: Utc::now(),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        assert_eq!(proj.epics.get("e-1").unwrap().total_tasks, 1);
        assert_eq!(proj.progress_pct("e-1"), Some(0.0));
    }

    #[test]
    fn is_blocked_checks_dependencies() {
        let mut proj = BacklogProjection::default();
        let now = Utc::now();

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Foundation".into(),
            description: "".into(),
            epic_id: None,
            created_at: now,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-2"),
            title: "Depends on t-1".into(),
            description: "".into(),
            epic_id: None,
            created_at: now,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![TaskId::new("t-1")],
        });

        assert!(!proj.is_blocked("t-1")); // no deps
        assert!(proj.is_blocked("t-2")); // t-1 not completed

        proj.apply_event(&RewindEvent::TaskCompleted {
            task_id: TaskId::new("t-1"),
            completed_at: now,
        });

        assert!(!proj.is_blocked("t-2")); // t-1 now completed
    }

    #[test]
    fn criteria_checked_count_tracks_progress() {
        let mut proj = BacklogProjection::default();
        let now = Utc::now();

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Task with criteria".into(),
            description: "".into(),
            epic_id: None,
            created_at: now,
            acceptance_criteria: vec![
                AcceptanceCriterion {
                    description: "Criterion A".into(),
                    checked: false,
                },
                AcceptanceCriterion {
                    description: "Criterion B".into(),
                    checked: false,
                },
                AcceptanceCriterion {
                    description: "Criterion C".into(),
                    checked: false,
                },
            ],
            story_type: None,
            depends_on: vec![],
        });

        assert_eq!(proj.criteria_checked_count("t-1"), (0, 3));

        proj.apply_event(&RewindEvent::CriterionChecked {
            task_id: TaskId::new("t-1"),
            criterion_index: 0,
            checked_at: now,
        });

        assert_eq!(proj.criteria_checked_count("t-1"), (1, 3));

        proj.apply_event(&RewindEvent::CriterionChecked {
            task_id: TaskId::new("t-1"),
            criterion_index: 2,
            checked_at: now,
        });

        assert_eq!(proj.criteria_checked_count("t-1"), (2, 3));
    }
}
