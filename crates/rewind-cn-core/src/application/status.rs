use std::collections::HashMap;

use serde::Serialize;

use crate::domain::model::{BacklogProjection, EpicProgressProjection};

#[derive(Debug, Serialize)]
pub struct StatusSummary {
    pub total_tasks: usize,
    pub by_status: HashMap<String, usize>,
    pub epics: Vec<EpicSummary>,
}

#[derive(Debug, Serialize)]
pub struct EpicSummary {
    pub epic_id: String,
    pub title: String,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub is_completed: bool,
}

pub fn build_summary(
    backlog: &BacklogProjection,
    epic_progress: &EpicProgressProjection,
) -> StatusSummary {
    let mut by_status: HashMap<String, usize> = HashMap::new();
    for task in backlog.tasks.values() {
        *by_status.entry(task.status.to_string()).or_insert(0) += 1;
    }

    let epics = epic_progress
        .epics
        .values()
        .map(|e| EpicSummary {
            epic_id: e.epic_id.to_string(),
            title: e.title.clone(),
            total_tasks: e.total_tasks,
            completed_tasks: e.completed_tasks,
            is_completed: e.is_completed,
        })
        .collect();

    StatusSummary {
        total_tasks: backlog.task_count(),
        by_status,
        epics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::RewindEvent;
    use crate::domain::ids::{EpicId, TaskId};
    use chrono::Utc;

    #[test]
    fn build_summary_counts_statuses() {
        let mut backlog = BacklogProjection::default();
        let mut epics = EpicProgressProjection::default();

        epics.apply_event(&RewindEvent::EpicCreated {
            epic_id: EpicId::new("e-1"),
            title: "Sprint 1".into(),
            description: "".into(),
            created_at: Utc::now(),
        });

        backlog.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Task 1".into(),
            description: "".into(),
            epic_id: Some(EpicId::new("e-1")),
            created_at: Utc::now(),
        });
        epics.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Task 1".into(),
            description: "".into(),
            epic_id: Some(EpicId::new("e-1")),
            created_at: Utc::now(),
        });

        backlog.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-2"),
            title: "Task 2".into(),
            description: "".into(),
            epic_id: None,
            created_at: Utc::now(),
        });

        backlog.apply_event(&RewindEvent::TaskCompleted {
            task_id: TaskId::new("t-2"),
            completed_at: Utc::now(),
        });

        let summary = build_summary(&backlog, &epics);
        assert_eq!(summary.total_tasks, 2);
        assert_eq!(summary.by_status.get("pending"), Some(&1));
        assert_eq!(summary.by_status.get("completed"), Some(&1));
        assert_eq!(summary.epics.len(), 1);
        assert_eq!(summary.epics[0].total_tasks, 1);
    }
}
