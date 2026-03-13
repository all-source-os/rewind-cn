//! Integration tests for progress notes query (US-012).

use rewind_cn_core::application::commands::{
    AssignTask, CompleteTask, CreateEpic, CreateTask, FailTask, StartTask,
};
use rewind_cn_core::domain::events::RewindEvent;
use rewind_cn_core::domain::ids::AgentId;
use rewind_cn_core::infrastructure::engine::RewindEngine;
use rewind_cn_core::infrastructure::toon;

#[tokio::test]
async fn progress_notes_returns_all_notes() {
    let engine = RewindEngine::in_memory().await;

    // Create epic and task
    let epic_events = engine
        .create_epic(CreateEpic {
            title: "Progress Test Epic".into(),
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
            title: "Progress task".into(),
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

    // Assign, start, and complete task (generates ProgressNoted events)
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
    engine
        .complete_task(CompleteTask {
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            discretionary_note: Some("Learned about error handling".into()),
        })
        .await
        .unwrap();

    // Query all progress notes
    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let notes = analytics.progress_notes(None, None);

    assert_eq!(notes.len(), 2, "Should have TaskCompleted + Discretionary notes");
    assert_eq!(format!("{:?}", notes[0].note_type), "TaskCompleted");
    assert_eq!(format!("{:?}", notes[1].note_type), "Discretionary");
    assert!(notes[1].note.contains("error handling"));
    assert!(notes[0].task_id.is_some());
}

#[tokio::test]
async fn progress_notes_filters_by_note_type() {
    let engine = RewindEngine::in_memory().await;

    let task_events = engine
        .create_task(CreateTask {
            title: "Filter task".into(),
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

    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

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

    // Fail the task (generates TaskFailed + optional Discretionary)
    engine
        .fail_task(FailTask {
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            reason: "Tests failed".into(),
            discretionary_note: Some("Need to fix imports".into()),
        })
        .await
        .unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    // Filter by TaskFailed
    let failed_notes = analytics.progress_notes(None, Some("TaskFailed"));
    assert_eq!(failed_notes.len(), 1);
    assert!(failed_notes[0].note.contains("failed"));

    // Filter by Discretionary
    let disc_notes = analytics.progress_notes(None, Some("Discretionary"));
    assert_eq!(disc_notes.len(), 1);
    assert!(disc_notes[0].note.contains("imports"));

    // Filter by non-existent type returns empty
    let empty = analytics.progress_notes(None, Some("NonExistent"));
    assert!(empty.is_empty());
}

#[tokio::test]
async fn progress_notes_filters_by_session_id() {
    let engine = RewindEngine::in_memory().await;

    // Two sessions
    let s1_events = engine.start_session().await.unwrap();
    let s1 = match &s1_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };
    let s2_events = engine.start_session().await.unwrap();
    let s2 = match &s2_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

    // Create tasks for each session
    let t1_events = engine
        .create_task(CreateTask {
            title: "Task S1".into(),
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
            title: "Task S2".into(),
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

    // Complete tasks in different sessions
    for (task_id, session_id) in [(t1, s1.clone()), (t2, s2.clone())] {
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
        engine
            .complete_task(CompleteTask {
                task_id,
                session_id,
                discretionary_note: None,
            })
            .await
            .unwrap();
    }

    let analytics = engine.analytics();
    let analytics = analytics.read().await;

    let s1_notes = analytics.progress_notes(Some(s1.as_ref()), None);
    assert_eq!(s1_notes.len(), 1);

    let s2_notes = analytics.progress_notes(Some(s2.as_ref()), None);
    assert_eq!(s2_notes.len(), 1);

    let all_notes = analytics.progress_notes(None, None);
    assert_eq!(all_notes.len(), 2);
}

#[tokio::test]
async fn progress_notes_toon_format() {
    let engine = RewindEngine::in_memory().await;

    let task_events = engine
        .create_task(CreateTask {
            title: "Toon task".into(),
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

    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

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
    engine
        .complete_task(CompleteTask {
            task_id,
            session_id,
            discretionary_note: Some("Cache invalidation trick".into()),
        })
        .await
        .unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let notes = analytics.progress_notes(None, None);

    let toon_output = toon::format_progress_list(&notes);
    assert!(toon_output.starts_with("[session_id|task_id|note_type|noted_at|note]\n"));
    assert!(toon_output.contains("|TaskCompleted|"));
    assert!(toon_output.contains("|Discretionary|"));
    assert!(toon_output.contains("Cache invalidation trick"));
}

#[tokio::test]
async fn progress_notes_survive_rebuild() {
    let engine = RewindEngine::in_memory().await;

    let task_events = engine
        .create_task(CreateTask {
            title: "Rebuild task".into(),
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

    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

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
    engine
        .complete_task(CompleteTask {
            task_id,
            session_id,
            discretionary_note: None,
        })
        .await
        .unwrap();

    // Rebuild projections
    engine.rebuild_projections().await.unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let notes = analytics.progress_notes(None, None);
    assert_eq!(notes.len(), 1, "Should survive rebuild");
}

#[tokio::test]
async fn progress_notes_without_task_id() {
    use rewind_cn_core::domain::events::ProgressNoteType;

    let engine = RewindEngine::in_memory().await;

    let session_events = engine.start_session().await.unwrap();
    let session_id = match &session_events[0] {
        RewindEvent::SessionStarted { session_id, .. } => session_id.clone(),
        _ => panic!("Expected SessionStarted"),
    };

    // Emit a ProgressNoted event with task_id: None
    engine
        .append_events(vec![RewindEvent::ProgressNoted {
            session_id: session_id.clone(),
            task_id: None,
            note: "Session-level observation".into(),
            note_type: ProgressNoteType::Discretionary,
            noted_at: chrono::Utc::now(),
        }])
        .await
        .unwrap();

    let analytics = engine.analytics();
    let analytics = analytics.read().await;
    let notes = analytics.progress_notes(None, None);

    assert_eq!(notes.len(), 1);
    assert!(notes[0].task_id.is_none(), "task_id should be None");
    assert_eq!(notes[0].note, "Session-level observation");
}
