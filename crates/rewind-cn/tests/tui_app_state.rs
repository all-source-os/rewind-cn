//! Tests for TUI App state machine.
//! Verifies that events correctly update the dashboard state.

// The tui module is private to the binary crate, so we test via
// the same event application logic duplicated here.
// This validates the state transition contract.

use chrono::Utc;
use rewind_cn_core::domain::events::{AcceptanceCriterion, RewindEvent};
use rewind_cn_core::domain::ids::{AgentId, EpicId, SessionId, TaskId};
use rewind_cn_core::domain::model::TaskStatus;

/// Minimal re-implementation of the TUI App state for testing.
/// Mirrors the real App's `apply_event` logic.
#[derive(Default)]
struct TestApp {
    task_count: usize,
    task_statuses: std::collections::HashMap<String, TaskStatus>,
    criteria_checked: std::collections::HashMap<String, usize>,
    criteria_total: std::collections::HashMap<String, usize>,
    tool_call_counts: std::collections::HashMap<String, usize>,
    epic_title: Option<String>,
    completed: usize,
    failed: usize,
    session_started: bool,
}

impl TestApp {
    fn apply(&mut self, event: &RewindEvent) {
        match event {
            RewindEvent::EpicCreated { title, .. } => {
                self.epic_title = Some(title.clone());
            }
            RewindEvent::TaskCreated {
                task_id,
                acceptance_criteria,
                ..
            } => {
                self.task_count += 1;
                self.task_statuses
                    .insert(task_id.to_string(), TaskStatus::Pending);
                self.criteria_total
                    .insert(task_id.to_string(), acceptance_criteria.len());
                self.criteria_checked.insert(task_id.to_string(), 0);
                self.tool_call_counts.insert(task_id.to_string(), 0);
            }
            RewindEvent::TaskAssigned { task_id, .. } => {
                self.task_statuses
                    .insert(task_id.to_string(), TaskStatus::Assigned);
            }
            RewindEvent::TaskStarted { task_id, .. } => {
                self.task_statuses
                    .insert(task_id.to_string(), TaskStatus::InProgress);
            }
            RewindEvent::TaskCompleted { task_id, .. } => {
                self.task_statuses
                    .insert(task_id.to_string(), TaskStatus::Completed);
                self.completed += 1;
            }
            RewindEvent::TaskFailed { task_id, .. } => {
                self.task_statuses
                    .insert(task_id.to_string(), TaskStatus::Failed);
                self.failed += 1;
            }
            RewindEvent::AgentToolCall { task_id, .. } => {
                *self
                    .tool_call_counts
                    .entry(task_id.to_string())
                    .or_insert(0) += 1;
            }
            RewindEvent::CriterionChecked { task_id, .. } => {
                *self
                    .criteria_checked
                    .entry(task_id.to_string())
                    .or_insert(0) += 1;
            }
            RewindEvent::SessionStarted { .. } => {
                self.session_started = true;
            }
            _ => {}
        }
    }
}

#[test]
fn tui_state_full_lifecycle() {
    let mut app = TestApp::default();
    let now = Utc::now();
    let epic_id = EpicId::new("e-1");
    let task_id = TaskId::new("t-1");

    // Epic created
    app.apply(&RewindEvent::EpicCreated {
        epic_id: epic_id.clone(),
        title: "My Feature".into(),
        description: "".into(),
        created_at: now,
        quality_gates: vec![],
    });
    assert_eq!(app.epic_title, Some("My Feature".into()));

    // Task created with 3 criteria
    app.apply(&RewindEvent::TaskCreated {
        task_id: task_id.clone(),
        title: "Do the thing".into(),
        description: "".into(),
        epic_id: Some(epic_id.clone()),
        created_at: now,
        acceptance_criteria: vec![
            AcceptanceCriterion {
                description: "A".into(),
                checked: false,
            },
            AcceptanceCriterion {
                description: "B".into(),
                checked: false,
            },
            AcceptanceCriterion {
                description: "C".into(),
                checked: false,
            },
        ],
        story_type: None,
        depends_on: vec![],
    });
    assert_eq!(app.task_count, 1);
    assert_eq!(app.task_statuses["t-1"], TaskStatus::Pending);
    assert_eq!(app.criteria_total["t-1"], 3);
    assert_eq!(app.criteria_checked["t-1"], 0);

    // Session starts
    app.apply(&RewindEvent::SessionStarted {
        session_id: SessionId::new("s-1"),
        started_at: now,
    });
    assert!(app.session_started);

    // Task assigned + started
    app.apply(&RewindEvent::TaskAssigned {
        task_id: task_id.clone(),
        agent_id: AgentId::new("agent-1"),
        assigned_at: now,
    });
    assert_eq!(app.task_statuses["t-1"], TaskStatus::Assigned);

    app.apply(&RewindEvent::TaskStarted {
        task_id: task_id.clone(),
        started_at: now,
    });
    assert_eq!(app.task_statuses["t-1"], TaskStatus::InProgress);

    // Tool calls
    for _ in 0..5 {
        app.apply(&RewindEvent::AgentToolCall {
            task_id: task_id.clone(),
            tool_name: "read_file".into(),
            args_summary: "".into(),
            result_summary: "".into(),
            called_at: now,
        });
    }
    assert_eq!(app.tool_call_counts["t-1"], 5);

    // Criteria checked
    app.apply(&RewindEvent::CriterionChecked {
        task_id: task_id.clone(),
        criterion_index: 0,
        checked_at: now,
    });
    app.apply(&RewindEvent::CriterionChecked {
        task_id: task_id.clone(),
        criterion_index: 1,
        checked_at: now,
    });
    assert_eq!(app.criteria_checked["t-1"], 2);

    // Task completed
    app.apply(&RewindEvent::TaskCompleted {
        task_id: task_id.clone(),
        completed_at: now,
    });
    assert_eq!(app.task_statuses["t-1"], TaskStatus::Completed);
    assert_eq!(app.completed, 1);
    assert_eq!(app.failed, 0);
}

#[test]
fn tui_state_failure_tracking() {
    let mut app = TestApp::default();
    let now = Utc::now();
    let task_id = TaskId::new("t-fail");

    app.apply(&RewindEvent::TaskCreated {
        task_id: task_id.clone(),
        title: "Failing task".into(),
        description: "".into(),
        epic_id: None,
        created_at: now,
        acceptance_criteria: vec![],
        story_type: None,
        depends_on: vec![],
    });

    app.apply(&RewindEvent::TaskStarted {
        task_id: task_id.clone(),
        started_at: now,
    });

    app.apply(&RewindEvent::TaskFailed {
        task_id: task_id.clone(),
        reason: "timeout".into(),
        failed_at: now,
    });

    assert_eq!(app.task_statuses["t-fail"], TaskStatus::Failed);
    assert_eq!(app.failed, 1);
    assert_eq!(app.completed, 0);
}

#[test]
fn tui_state_multiple_tasks() {
    let mut app = TestApp::default();
    let now = Utc::now();

    for i in 0..5 {
        app.apply(&RewindEvent::TaskCreated {
            task_id: TaskId::new(format!("t-{i}")),
            title: format!("Task {i}"),
            description: "".into(),
            epic_id: None,
            created_at: now,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });
    }

    assert_eq!(app.task_count, 5);
    assert_eq!(app.task_statuses.len(), 5);

    // Complete some, fail one
    app.apply(&RewindEvent::TaskCompleted {
        task_id: TaskId::new("t-0"),
        completed_at: now,
    });
    app.apply(&RewindEvent::TaskCompleted {
        task_id: TaskId::new("t-1"),
        completed_at: now,
    });
    app.apply(&RewindEvent::TaskFailed {
        task_id: TaskId::new("t-2"),
        reason: "err".into(),
        failed_at: now,
    });

    assert_eq!(app.completed, 2);
    assert_eq!(app.failed, 1);
}
