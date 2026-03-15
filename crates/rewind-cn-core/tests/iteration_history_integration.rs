//! Integration tests for iteration history query (US-011).

use rewind_cn_core::application::commands::{AssignTask, CreateEpic, CreateTask, StartTask};
use rewind_cn_core::domain::events::RewindEvent;
use rewind_cn_core::domain::ids::AgentId;
use rewind_cn_core::infrastructure::engine::RewindEngine;
use rewind_cn_core::infrastructure::toon;

#[tokio::test]
async fn iteration_history_returns_correct_data_for_session() {
    let engine = RewindEngine::in_memory().await;

    // Create epic and task
    let epic_events = engine
        .create_epic(CreateEpic {
            title: "Iteration Test Epic".into(),
            description: "".into(),
            quality_gates: vec![],
        })
        .await
        .unwrap();
    let epic_id = match &epic_events[0] {
        RewindEvent::EpicCreated { epic_id, .. } => epic_id.clone(),
        _ => panic!("Expected EpicCreated"),
    };

    let task_events = engine
        .create_task(CreateTask {
            title: "Iteration task".into(),
            description: "".into(),
            epic_id: Some(epic_id),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();
    let task_id = match &task_events[0] {
        RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
        _ => panic!("Expected TaskCreated"),
    };

    // Start session
    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

    // Assign and start task
    engine
        .assign_task(AssignTask {
            task_id: task_id.clone(),
            agent_id: AgentId::new("agent-1"),
        })
        .await
        .unwrap();
    engine
        .start_task(StartTask {
            task_id: task_id.clone(),
        })
        .await
        .unwrap();

    // Emit multiple iteration logs
    engine
        .append_events(vec![
            RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: task_id.clone(),
                iteration_number: 1,
                agent_output: "Read the handler file and identified the bug".into(),
                duration_ms: 3200,
            },
            RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: task_id.clone(),
                iteration_number: 2,
                agent_output: "Applied fix to handler.rs and ran tests".into(),
                duration_ms: 4500,
            },
            RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: task_id.clone(),
                iteration_number: 3,
                agent_output: "All tests passing, committing changes".into(),
                duration_ms: 1800,
            },
        ])
        .await
        .unwrap();

    // Query iteration history via analytics
    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let iterations = analytics.iteration_history(session_id.as_ref());

    assert_eq!(iterations.len(), 3, "Should have 3 iterations");
    assert_eq!(iterations[0].iteration_number, 1);
    assert_eq!(iterations[1].iteration_number, 2);
    assert_eq!(iterations[2].iteration_number, 3);
    assert_eq!(iterations[0].duration_ms, 3200);
    assert_eq!(iterations[1].duration_ms, 4500);
    assert_eq!(iterations[2].duration_ms, 1800);
    assert!(iterations[0].agent_output.contains("identified the bug"));

    // Verify TOON format
    let toon_output = toon::format_iteration_list(&iterations);
    assert!(toon_output.starts_with("[iter|task_id|duration_ms|output]\n"));
    assert!(toon_output.contains("|3200|"));
    assert!(toon_output.contains("|4500|"));
    assert!(toon_output.contains("|1800|"));

    // Verify empty session returns empty
    let empty = analytics.iteration_history("nonexistent-session");
    assert!(empty.is_empty());
}

#[tokio::test]
async fn iteration_history_survives_rebuild() {
    let engine = RewindEngine::in_memory().await;

    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

    let task_events = engine
        .create_task(CreateTask {
            title: "Rebuild test".into(),
            description: "".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();
    let task_id = match &task_events[0] {
        RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
        _ => panic!("Expected TaskCreated"),
    };

    engine
        .append_events(vec![RewindEvent::IterationLogged {
            session_id: session_id.clone(),
            task_id: task_id.clone(),
            iteration_number: 1,
            agent_output: "First iteration".into(),
            duration_ms: 2000,
        }])
        .await
        .unwrap();

    // Rebuild projections
    engine.rebuild_projections().await.unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let iterations = analytics.iteration_history(session_id.as_ref());
    assert_eq!(iterations.len(), 1, "Should survive rebuild");
    assert_eq!(iterations[0].iteration_number, 1);
}

#[tokio::test]
async fn iteration_history_multiple_tasks_in_same_session() {
    let engine = RewindEngine::in_memory().await;

    // Start session
    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

    // Create two tasks
    let t1_events = engine
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
    let t1 = match &t1_events[0] {
        RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
        _ => panic!("Expected TaskCreated"),
    };

    let t2_events = engine
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
    let t2 = match &t2_events[0] {
        RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
        _ => panic!("Expected TaskCreated"),
    };

    // Log iterations for both tasks under same session
    engine
        .append_events(vec![
            RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: t1.clone(),
                iteration_number: 1,
                agent_output: "Task A iteration 1".into(),
                duration_ms: 1000,
            },
            RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: t2.clone(),
                iteration_number: 1,
                agent_output: "Task B iteration 1".into(),
                duration_ms: 2000,
            },
            RewindEvent::IterationLogged {
                session_id: session_id.clone(),
                task_id: t1.clone(),
                iteration_number: 2,
                agent_output: "Task A iteration 2".into(),
                duration_ms: 1500,
            },
        ])
        .await
        .unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let iterations = analytics.iteration_history(session_id.as_ref());

    assert_eq!(
        iterations.len(),
        3,
        "Should have iterations from both tasks"
    );
    // Verify both task IDs are present
    let task_ids: Vec<String> = iterations.iter().map(|i| i.task_id.to_string()).collect();
    assert!(task_ids.contains(&t1.to_string()));
    assert!(task_ids.contains(&t2.to_string()));
}
