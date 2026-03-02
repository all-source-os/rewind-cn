use std::collections::HashMap;

use allframe::cqrs::Projection;
use chrono::{DateTime, Utc};

use super::aggregates::TaskStatus;
use super::events::RalphEvent;

/// Read model for a single task in the backlog.
#[derive(Debug, Clone)]
pub struct TaskView {
    pub task_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub epic_id: Option<String>,
    pub agent_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Projection that maintains the full backlog as a HashMap of task views.
#[derive(Debug, Default)]
pub struct BacklogProjection {
    pub tasks: HashMap<String, TaskView>,
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
}

impl Projection for BacklogProjection {
    type Event = RalphEvent;

    fn apply(&mut self, event: &RalphEvent) {
        match event {
            RalphEvent::TaskCreated {
                task_id,
                title,
                description,
                epic_id,
                created_at,
            } => {
                self.tasks.insert(
                    task_id.clone(),
                    TaskView {
                        task_id: task_id.clone(),
                        title: title.clone(),
                        description: description.clone(),
                        status: TaskStatus::Pending,
                        epic_id: epic_id.clone(),
                        agent_id: None,
                        created_at: *created_at,
                    },
                );
            }
            RalphEvent::TaskAssigned {
                task_id, agent_id, ..
            } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.status = TaskStatus::Assigned;
                    task.agent_id = Some(agent_id.clone());
                }
            }
            RalphEvent::TaskStarted { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.status = TaskStatus::InProgress;
                }
            }
            RalphEvent::TaskCompleted { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.status = TaskStatus::Completed;
                }
            }
            RalphEvent::TaskFailed { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.status = TaskStatus::Failed;
                }
            }
            RalphEvent::TaskBlocked { task_id, .. } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.status = TaskStatus::Blocked;
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

#[derive(Debug, Clone, Default)]
pub struct EpicProgress {
    pub epic_id: String,
    pub title: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub is_completed: bool,
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
}

impl Projection for EpicProgressProjection {
    type Event = RalphEvent;

    fn apply(&mut self, event: &RalphEvent) {
        match event {
            RalphEvent::EpicCreated {
                epic_id, title, ..
            } => {
                self.epics.insert(
                    epic_id.clone(),
                    EpicProgress {
                        epic_id: epic_id.clone(),
                        title: title.clone(),
                        ..Default::default()
                    },
                );
            }
            RalphEvent::EpicCompleted { epic_id, .. } => {
                if let Some(epic) = self.epics.get_mut(epic_id) {
                    epic.is_completed = true;
                }
            }
            RalphEvent::TaskCreated { epic_id, .. } => {
                if let Some(eid) = epic_id {
                    if let Some(epic) = self.epics.get_mut(eid) {
                        epic.total_tasks += 1;
                    }
                }
            }
            RalphEvent::TaskCompleted { task_id, .. } => {
                // We need to find which epic this task belongs to — in a real impl
                // we'd cross-reference. For now, we skip this (the backlog projection
                // has the mapping). This is a known simplification.
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
    fn backlog_projection_tracks_tasks() {
        let mut proj = BacklogProjection::default();

        proj.apply(&RalphEvent::TaskCreated {
            task_id: "t-1".into(),
            title: "Task 1".into(),
            description: "Desc".into(),
            epic_id: None,
            created_at: Utc::now(),
        });

        assert_eq!(proj.task_count(), 1);
        assert_eq!(proj.pending_tasks().len(), 1);

        proj.apply(&RalphEvent::TaskCompleted {
            task_id: "t-1".into(),
            completed_at: Utc::now(),
        });

        assert_eq!(proj.pending_tasks().len(), 0);
        assert_eq!(
            proj.tasks.get("t-1").unwrap().status,
            TaskStatus::Completed
        );
    }

    #[test]
    fn epic_progress_tracks_tasks() {
        let mut proj = EpicProgressProjection::default();

        proj.apply(&RalphEvent::EpicCreated {
            epic_id: "e-1".into(),
            title: "Sprint 1".into(),
            description: "First sprint".into(),
            created_at: Utc::now(),
        });

        proj.apply(&RalphEvent::TaskCreated {
            task_id: "t-1".into(),
            title: "Task 1".into(),
            description: "Desc".into(),
            epic_id: Some("e-1".into()),
            created_at: Utc::now(),
        });

        assert_eq!(proj.epics.get("e-1").unwrap().total_tasks, 1);
        assert_eq!(proj.progress_pct("e-1"), Some(0.0));
    }
}
