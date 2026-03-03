use crate::domain::model::{BacklogProjection, TaskStatus, TaskView};

/// Returns pending tasks sorted by creation time (FIFO), up to `max`.
pub fn pick_runnable_tasks(backlog: &BacklogProjection, max: usize) -> Vec<&TaskView> {
    let mut pending: Vec<&TaskView> = backlog
        .tasks
        .values()
        .filter(|t| t.status == TaskStatus::Pending)
        .collect();
    pending.sort_by_key(|t| t.created_at);
    pending.truncate(max);
    pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::RalphEvent;
    use crate::domain::ids::TaskId;
    use chrono::{Duration, Utc};

    #[test]
    fn empty_backlog_returns_empty() {
        let backlog = BacklogProjection::default();
        assert!(pick_runnable_tasks(&backlog, 10).is_empty());
    }

    #[test]
    fn filters_non_pending() {
        let mut backlog = BacklogProjection::default();
        let now = Utc::now();

        backlog.apply_event(&RalphEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "Task 1".into(),
            description: "".into(),
            epic_id: None,
            created_at: now,
        });
        backlog.apply_event(&RalphEvent::TaskCreated {
            task_id: TaskId::new("t-2"),
            title: "Task 2".into(),
            description: "".into(),
            epic_id: None,
            created_at: now + Duration::seconds(1),
        });
        backlog.apply_event(&RalphEvent::TaskCompleted {
            task_id: TaskId::new("t-1"),
            completed_at: now,
        });

        let runnable = pick_runnable_tasks(&backlog, 10);
        assert_eq!(runnable.len(), 1);
        assert_eq!(runnable[0].task_id, TaskId::new("t-2"));
    }

    #[test]
    fn respects_max_limit() {
        let mut backlog = BacklogProjection::default();
        let now = Utc::now();

        for i in 0..5 {
            backlog.apply_event(&RalphEvent::TaskCreated {
                task_id: TaskId::new(format!("t-{i}")),
                title: format!("Task {i}"),
                description: "".into(),
                epic_id: None,
                created_at: now + Duration::seconds(i),
            });
        }

        assert_eq!(pick_runnable_tasks(&backlog, 2).len(), 2);
    }
}
