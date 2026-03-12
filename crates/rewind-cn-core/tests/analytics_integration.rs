//! Integration tests for analytics projection through the engine.

use rewind_cn_core::application::commands::{
    AssignTask, CompleteTask, CreateEpic, CreateTask, EndSession, FailTask, StartTask,
};
use rewind_cn_core::domain::events::{AcceptanceCriterion, GateTier, QualityGate, RewindEvent};
use rewind_cn_core::domain::ids::{AgentId, SessionId};
use rewind_cn_core::infrastructure::engine::RewindEngine;

use chrono::Utc;

/// Build a fully populated engine with an epic, multiple tasks in various states,
/// tool calls, and criteria checks. Returns the engine for query testing.
async fn populated_engine() -> RewindEngine<allframe::cqrs::InMemoryBackend<RewindEvent>> {
    let engine = RewindEngine::in_memory().await;

    // Create epic with quality gates
    engine
        .create_epic(CreateEpic {
            title: "Test Feature".into(),
            description: "A feature for testing analytics".into(),
            quality_gates: vec![
                QualityGate {
                    command: "cargo test".into(),
                    tier: GateTier::Epic,
                },
                QualityGate {
                    command: "cargo clippy".into(),
                    tier: GateTier::Epic,
                },
            ],
        })
        .await
        .unwrap();

    // Get epic ID from projection
    let epic_id = {
        let progress = engine.epic_progress();
        let progress = progress.read().await;
        progress.epics.values().next().unwrap().epic_id.clone()
    };

    // Create 3 tasks
    let mut task_ids = Vec::new();
    for (i, (title, criteria_count)) in [
        ("Schema migration", 2),
        ("Backend endpoint", 3),
        ("UI component", 1),
    ]
    .iter()
    .enumerate()
    {
        let criteria = (0..*criteria_count)
            .map(|j| AcceptanceCriterion {
                description: format!("Criterion {j} for task {i}"),
                checked: false,
            })
            .collect();

        let events = engine
            .create_task(CreateTask {
                title: title.to_string(),
                description: format!("Description for {title}"),
                epic_id: Some(epic_id.clone()),
                acceptance_criteria: criteria,
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        let task_id = match &events[0] {
            RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
            _ => panic!("Expected TaskCreated"),
        };
        task_ids.push(task_id);
    }

    // Start a session
    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

    // Task 0: assign, start, tool calls, criteria check, complete
    engine
        .assign_task(AssignTask {
            task_id: task_ids[0].clone(),
            agent_id: AgentId::new("agent-1"),
        })
        .await
        .unwrap();

    engine
        .start_task(StartTask {
            task_id: task_ids[0].clone(),
        })
        .await
        .unwrap();

    // Emit tool calls
    engine
        .append_events(vec![
            RewindEvent::AgentToolCall {
                task_id: task_ids[0].clone(),
                tool_name: "read_file".into(),
                args_summary: "schema.sql".into(),
                result_summary: "200 bytes".into(),
                called_at: Utc::now(),
            },
            RewindEvent::AgentToolCall {
                task_id: task_ids[0].clone(),
                tool_name: "write_file".into(),
                args_summary: "migration.sql".into(),
                result_summary: "ok".into(),
                called_at: Utc::now(),
            },
            RewindEvent::AgentToolCall {
                task_id: task_ids[0].clone(),
                tool_name: "run_command".into(),
                args_summary: "cargo test".into(),
                result_summary: "exit 0".into(),
                called_at: Utc::now(),
            },
        ])
        .await
        .unwrap();

    // Check criteria
    engine
        .append_events(vec![
            RewindEvent::CriterionChecked {
                task_id: task_ids[0].clone(),
                criterion_index: 0,
                checked_at: Utc::now(),
            },
            RewindEvent::CriterionChecked {
                task_id: task_ids[0].clone(),
                criterion_index: 1,
                checked_at: Utc::now(),
            },
        ])
        .await
        .unwrap();

    engine
        .complete_task(CompleteTask {
            task_id: task_ids[0].clone(),
            session_id: SessionId::generate(),
            discretionary_note: None,
        })
        .await
        .unwrap();

    // Task 1: assign, start, fail
    engine
        .assign_task(AssignTask {
            task_id: task_ids[1].clone(),
            agent_id: AgentId::new("agent-1"),
        })
        .await
        .unwrap();

    engine
        .start_task(StartTask {
            task_id: task_ids[1].clone(),
        })
        .await
        .unwrap();

    engine
        .append_events(vec![RewindEvent::AgentToolCall {
            task_id: task_ids[1].clone(),
            tool_name: "read_file".into(),
            args_summary: "endpoint.rs".into(),
            result_summary: "300 bytes".into(),
            called_at: Utc::now(),
        }])
        .await
        .unwrap();

    engine
        .fail_task(FailTask {
            task_id: task_ids[1].clone(),
            session_id: SessionId::generate(),
            reason: "Tests failed: 2 assertions".into(),
            discretionary_note: None,
        })
        .await
        .unwrap();

    // Task 2: still pending (not started)

    // End session
    engine.end_session(EndSession { session_id }).await.unwrap();

    engine
}

#[tokio::test]
async fn analytics_task_summary_shows_all_tasks() {
    let engine = populated_engine().await;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let tasks = analytics.task_summary(None);

    assert_eq!(tasks.len(), 3, "Should have 3 tasks");
}

#[tokio::test]
async fn analytics_task_outcomes_correct() {
    use rewind_cn_core::application::analytics::TaskOutcome;

    let engine = populated_engine().await;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    let task_map: std::collections::HashMap<_, _> = analytics
        .tasks
        .values()
        .map(|t| (t.title.as_str(), t))
        .collect();

    assert_eq!(
        task_map["Schema migration"].outcome,
        TaskOutcome::Passed,
        "Completed task should be Passed"
    );
    assert_eq!(
        task_map["Backend endpoint"].outcome,
        TaskOutcome::Failed,
        "Failed task should be Failed"
    );
    assert_eq!(
        task_map["UI component"].outcome,
        TaskOutcome::Pending,
        "Unstarted task should be Pending"
    );
}

#[tokio::test]
async fn analytics_tool_call_counts_correct() {
    let engine = populated_engine().await;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    let task_map: std::collections::HashMap<_, _> = analytics
        .tasks
        .values()
        .map(|t| (t.title.as_str(), t))
        .collect();

    assert_eq!(
        task_map["Schema migration"].tool_call_count, 3,
        "Should have 3 tool calls"
    );
    assert_eq!(
        task_map["Backend endpoint"].tool_call_count, 1,
        "Should have 1 tool call"
    );
    assert_eq!(
        task_map["UI component"].tool_call_count, 0,
        "Should have 0 tool calls"
    );
}

#[tokio::test]
async fn analytics_criteria_tracking_correct() {
    let engine = populated_engine().await;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    let task_map: std::collections::HashMap<_, _> = analytics
        .tasks
        .values()
        .map(|t| (t.title.as_str(), t))
        .collect();

    let schema = task_map["Schema migration"];
    assert_eq!(schema.criteria_total, 2);
    assert_eq!(
        schema.criteria_checked, 2,
        "Both criteria should be checked"
    );

    let backend = task_map["Backend endpoint"];
    assert_eq!(backend.criteria_total, 3);
    assert_eq!(
        backend.criteria_checked, 0,
        "No criteria checked on failed task"
    );
}

#[tokio::test]
async fn analytics_epic_summary_aggregates() {
    let engine = populated_engine().await;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let epics = analytics.epic_summary();

    assert_eq!(epics.len(), 1);
    let epic = epics[0];
    assert_eq!(epic.title, "Test Feature");
    assert_eq!(epic.total_tasks, 3);
    assert_eq!(epic.completed, 1);
    assert_eq!(epic.failed, 1);
    assert_eq!(epic.gates_total, 2);
}

#[tokio::test]
async fn analytics_tool_usage_ranked() {
    let engine = populated_engine().await;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let usage = analytics.tool_usage();

    assert!(!usage.is_empty());
    // read_file should be most used (2 calls: 1 from task 0, 1 from task 1)
    assert_eq!(usage[0].tool_name, "read_file");
    assert_eq!(usage[0].call_count, 2);
}

#[tokio::test]
async fn analytics_session_history_has_duration() {
    let engine = populated_engine().await;

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let sessions = analytics.session_history();

    assert_eq!(sessions.len(), 1);
    assert!(
        sessions[0].duration_secs.is_some(),
        "Ended session should have duration"
    );
    assert!(
        sessions[0].ended_at.is_some(),
        "Ended session should have end time"
    );
}

#[tokio::test]
async fn analytics_epic_filter_narrows_results() {
    let engine = populated_engine().await;

    // Create a task outside the epic
    engine
        .create_task(CreateTask {
            title: "Orphan task".into(),
            description: "No epic".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    let all_tasks = analytics.task_summary(None);
    assert_eq!(all_tasks.len(), 4, "Should have 4 total tasks");

    // Get epic ID
    let epic_id = analytics.epics.keys().next().unwrap();
    let filtered = analytics.task_summary(Some(epic_id));
    assert_eq!(filtered.len(), 3, "Should have 3 tasks in epic");
}

#[tokio::test]
async fn analytics_survives_rebuild() {
    let engine = populated_engine().await;

    // Rebuild projections from event store
    engine.rebuild_projections().await.unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    // Should still have all data after rebuild
    assert_eq!(analytics.tasks.len(), 3);
    assert_eq!(analytics.epics.len(), 1);
    assert_eq!(analytics.sessions.len(), 1);
    assert!(analytics.tool_counts.len() >= 2);
}
