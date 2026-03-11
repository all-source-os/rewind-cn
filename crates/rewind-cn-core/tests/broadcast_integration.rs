//! Integration tests for the engine's broadcast channel.
//! Verifies that events are delivered to subscribers in real-time.

use rewind_cn_core::application::commands::{CreateEpic, CreateTask};
use rewind_cn_core::domain::events::RewindEvent;
use rewind_cn_core::infrastructure::engine::RewindEngine;

#[tokio::test]
async fn subscriber_receives_events() {
    let engine = RewindEngine::in_memory().await;
    let mut rx = engine.subscribe();

    // Create an epic
    engine
        .create_epic(CreateEpic {
            title: "Broadcast Test".into(),
            description: "".into(),
            quality_gates: vec![],
        })
        .await
        .unwrap();

    // Subscriber should receive the EpicCreated event
    let event = rx.try_recv().unwrap();
    match event {
        RewindEvent::EpicCreated { title, .. } => {
            assert_eq!(title, "Broadcast Test");
        }
        _ => panic!("Expected EpicCreated, got {:?}", event),
    }
}

#[tokio::test]
async fn subscriber_receives_multiple_events_in_order() {
    let engine = RewindEngine::in_memory().await;
    let mut rx = engine.subscribe();

    engine
        .create_task(CreateTask {
            title: "Task A".into(),
            description: "".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();

    engine
        .create_task(CreateTask {
            title: "Task B".into(),
            description: "".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();

    let event1 = rx.try_recv().unwrap();
    let event2 = rx.try_recv().unwrap();

    match (&event1, &event2) {
        (
            RewindEvent::TaskCreated { title: t1, .. },
            RewindEvent::TaskCreated { title: t2, .. },
        ) => {
            assert_eq!(t1, "Task A");
            assert_eq!(t2, "Task B");
        }
        _ => panic!("Expected two TaskCreated events"),
    }
}

#[tokio::test]
async fn no_subscriber_does_not_block() {
    let engine = RewindEngine::in_memory().await;
    // No subscriber — this should not panic or block
    engine
        .create_task(CreateTask {
            title: "Unobserved".into(),
            description: "".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();

    // Now subscribe and verify no events queued
    let mut rx = engine.subscribe();
    assert!(
        rx.try_recv().is_err(),
        "New subscriber should have no backlog"
    );
}

#[tokio::test]
async fn multiple_subscribers_all_receive() {
    let engine = RewindEngine::in_memory().await;
    let mut rx1 = engine.subscribe();
    let mut rx2 = engine.subscribe();

    engine
        .create_task(CreateTask {
            title: "Shared event".into(),
            description: "".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();

    let e1 = rx1.try_recv().unwrap();
    let e2 = rx2.try_recv().unwrap();

    match (&e1, &e2) {
        (
            RewindEvent::TaskCreated { title: t1, .. },
            RewindEvent::TaskCreated { title: t2, .. },
        ) => {
            assert_eq!(t1, "Shared event");
            assert_eq!(t2, "Shared event");
        }
        _ => panic!("Both subscribers should receive the same event"),
    }
}

#[tokio::test]
async fn append_events_also_broadcasts() {
    let engine = RewindEngine::in_memory().await;
    let mut rx = engine.subscribe();

    use chrono::Utc;
    use rewind_cn_core::domain::ids::TaskId;

    engine
        .append_events(vec![RewindEvent::AgentToolCall {
            task_id: TaskId::new("t-1"),
            tool_name: "read_file".into(),
            args_summary: "main.rs".into(),
            result_summary: "ok".into(),
            called_at: Utc::now(),
        }])
        .await
        .unwrap();

    let event = rx.try_recv().unwrap();
    match event {
        RewindEvent::AgentToolCall { tool_name, .. } => {
            assert_eq!(tool_name, "read_file");
        }
        _ => panic!("Expected AgentToolCall"),
    }
}
