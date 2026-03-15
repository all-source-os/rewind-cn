use std::path::Path;
use std::sync::Arc;

use allframe::cqrs::allsource_backend::AllSourceBackend;
use allframe::cqrs::{CommandBus, EventStore, InMemoryBackend, Projection};
use tokio::sync::{broadcast, RwLock};
use tracing::info;

use crate::application::analytics::AnalyticsProjection;
use crate::application::commands;
use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;
use crate::domain::model::{BacklogProjection, EpicProgressProjection};

use super::command_bridge::*;

/// Central engine that wires together EventStore, CommandBus, and Projections.
pub struct RewindEngine<
    B: allframe::cqrs::EventStoreBackend<RewindEvent> = AllSourceBackend<RewindEvent>,
> {
    pub event_store: Arc<EventStore<RewindEvent, B>>,
    pub command_bus: CommandBus<RewindEvent>,
    backlog: Arc<RwLock<BacklogProjection>>,
    epic_progress: Arc<RwLock<EpicProgressProjection>>,
    analytics: Arc<RwLock<AnalyticsProjection>>,
    event_tx: broadcast::Sender<RewindEvent>,
}

impl RewindEngine<AllSourceBackend<RewindEvent>> {
    /// Initialize a new engine with persistent storage at the given path.
    pub async fn init(data_path: &str) -> Result<Self, RewindError> {
        let path = Path::new(data_path);
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(|e| {
                RewindError::Storage(format!("Failed to create data directory: {e}"))
            })?;
        }

        let backend = AllSourceBackend::production(data_path).map_err(RewindError::Storage)?;
        let event_store = Arc::new(EventStore::with_backend(backend));
        let command_bus = CommandBus::new();
        let (event_tx, _) = broadcast::channel(256);

        let mut engine = Self {
            event_store,
            command_bus,
            backlog: Arc::new(RwLock::new(BacklogProjection::default())),
            epic_progress: Arc::new(RwLock::new(EpicProgressProjection::default())),
            analytics: Arc::new(RwLock::new(AnalyticsProjection::default())),
            event_tx,
        };
        engine.register_handlers().await;

        info!("Rewind engine initialized at {data_path}");
        Ok(engine)
    }

    /// Load an existing engine from a data path.
    pub async fn load(data_path: &str) -> Result<Self, RewindError> {
        if !Path::new(data_path).exists() {
            return Err(RewindError::NotFound(format!(
                "Data directory does not exist: {data_path}. Run `rewind init` first."
            )));
        }
        Self::init(data_path).await
    }
}

impl RewindEngine<InMemoryBackend<RewindEvent>> {
    /// Create an in-memory engine for testing.
    pub async fn in_memory() -> Self {
        let event_store = Arc::new(EventStore::new());
        let command_bus = CommandBus::new();
        let (event_tx, _) = broadcast::channel(256);

        let mut engine = Self {
            event_store,
            command_bus,
            backlog: Arc::new(RwLock::new(BacklogProjection::default())),
            epic_progress: Arc::new(RwLock::new(EpicProgressProjection::default())),
            analytics: Arc::new(RwLock::new(AnalyticsProjection::default())),
            event_tx,
        };
        engine.register_handlers().await;
        engine
    }
}

impl<B: allframe::cqrs::EventStoreBackend<RewindEvent>> RewindEngine<B> {
    async fn register_handlers(&mut self) {
        self.command_bus
            .register::<CreateTaskCmd, _>(CreateTaskBridge)
            .await;
        self.command_bus
            .register::<AssignTaskCmd, _>(AssignTaskBridge)
            .await;
        self.command_bus
            .register::<StartTaskCmd, _>(StartTaskBridge)
            .await;
        self.command_bus
            .register::<CompleteTaskCmd, _>(CompleteTaskBridge)
            .await;
        self.command_bus
            .register::<FailTaskCmd, _>(FailTaskBridge)
            .await;
        self.command_bus
            .register::<RetryTaskCmd, _>(RetryTaskBridge)
            .await;
        self.command_bus
            .register::<CreateEpicCmd, _>(CreateEpicBridge)
            .await;
        self.command_bus
            .register::<CompleteEpicCmd, _>(CompleteEpicBridge)
            .await;
        self.command_bus
            .register::<StartSessionCmd, _>(StartSessionBridge)
            .await;
        self.command_bus
            .register::<EndSessionCmd, _>(EndSessionBridge)
            .await;
    }

    /// Apply an event to all projections and broadcast to subscribers.
    #[hotpath::measure]
    async fn apply_to_projections(&self, event: &RewindEvent) {
        self.backlog.write().await.apply(event);
        self.epic_progress.write().await.apply(event);
        self.analytics.write().await.apply_event(event);
        // Best-effort broadcast — no subscribers is fine
        let _ = self.event_tx.send(event.clone());
    }

    /// Subscribe to real-time event updates (for TUI dashboard).
    pub fn subscribe(&self) -> broadcast::Receiver<RewindEvent> {
        self.event_tx.subscribe()
    }

    /// Get a read handle to the backlog projection.
    pub fn backlog(&self) -> Arc<RwLock<BacklogProjection>> {
        self.backlog.clone()
    }

    /// Get a read handle to the epic progress projection.
    pub fn epic_progress(&self) -> Arc<RwLock<EpicProgressProjection>> {
        self.epic_progress.clone()
    }

    /// Get a read handle to the analytics projection.
    pub fn analytics(&self) -> Arc<RwLock<AnalyticsProjection>> {
        self.analytics.clone()
    }

    pub async fn create_task(
        &self,
        cmd: commands::CreateTask,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        self.dispatch_and_append("task", CreateTaskCmd(cmd)).await
    }

    pub async fn assign_task(
        &self,
        cmd: commands::AssignTask,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, AssignTaskCmd(cmd)).await
    }

    pub async fn start_task(
        &self,
        cmd: commands::StartTask,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, StartTaskCmd(cmd)).await
    }

    pub async fn complete_task(
        &self,
        cmd: commands::CompleteTask,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, CompleteTaskCmd(cmd))
            .await
    }

    pub async fn fail_task(
        &self,
        cmd: commands::FailTask,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, FailTaskCmd(cmd)).await
    }

    pub async fn retry_task(
        &self,
        cmd: commands::RetryTask,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, RetryTaskCmd(cmd)).await
    }

    pub async fn create_epic(
        &self,
        cmd: commands::CreateEpic,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        self.dispatch_and_append("epic", CreateEpicCmd(cmd)).await
    }

    pub async fn complete_epic(
        &self,
        cmd: commands::CompleteEpic,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let agg_id = cmd.epic_id.to_string();
        self.dispatch_and_append(&agg_id, CompleteEpicCmd(cmd))
            .await
    }

    pub async fn start_session(&self) -> Result<Vec<RewindEvent>, RewindError> {
        self.dispatch_and_append("session", StartSessionCmd(commands::StartSession))
            .await
    }

    pub async fn end_session(
        &self,
        cmd: commands::EndSession,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let agg_id = cmd.session_id.to_string();
        self.dispatch_and_append(&agg_id, EndSessionCmd(cmd)).await
    }

    /// Append events directly (without going through the command bus).
    /// Used for events like AgentToolCall and CriterionChecked that don't need command validation.
    #[hotpath::measure]
    pub async fn append_events(&self, events: Vec<RewindEvent>) -> Result<(), RewindError> {
        self.event_store
            .append("agent", events.clone())
            .await
            .map_err(RewindError::Storage)?;
        for event in &events {
            self.apply_to_projections(event).await;
        }
        Ok(())
    }

    /// Generic dispatch: dispatches a command, appends resulting events, and updates projections.
    #[hotpath::measure]
    async fn dispatch_and_append<C: allframe::cqrs::Command>(
        &self,
        aggregate_id: &str,
        command: C,
    ) -> Result<Vec<RewindEvent>, RewindError> {
        let events = self
            .command_bus
            .dispatch::<C>(command)
            .await
            .map_err(|e| RewindError::InvalidState(e.to_string()))?;
        self.event_store
            .append(aggregate_id, events.clone())
            .await
            .map_err(RewindError::Storage)?;
        for event in &events {
            self.apply_to_projections(event).await;
        }
        Ok(events)
    }

    /// Rebuild all projections from the event store.
    #[hotpath::measure]
    pub async fn rebuild_projections(&self) -> Result<(), RewindError> {
        let events = self
            .event_store
            .get_all_events()
            .await
            .map_err(RewindError::Storage)?;

        let mut backlog = self.backlog.write().await;
        let mut epic_progress = self.epic_progress.write().await;
        let mut analytics = self.analytics.write().await;
        *backlog = BacklogProjection::default();
        *epic_progress = EpicProgressProjection::default();
        *analytics = AnalyticsProjection::default();

        for event in &events {
            backlog.apply(event);
            epic_progress.apply(event);
            analytics.apply_event(event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::commands::{AssignTask, CompleteTask, CreateTask, StartTask};
    use crate::domain::ids::{AgentId, SessionId};
    use crate::domain::model::TaskStatus;

    #[tokio::test]
    async fn in_memory_engine_roundtrip() {
        let engine = RewindEngine::in_memory().await;

        let events = engine
            .create_task(CreateTask {
                title: "Write tests".into(),
                description: "Add unit tests".into(),
                epic_id: None,
                acceptance_criteria: vec![],
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        assert_eq!(events.len(), 1);

        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        assert_eq!(backlog.task_count(), 1);
    }

    #[tokio::test]
    async fn rebuild_projections_replays_events() {
        let engine = RewindEngine::in_memory().await;

        engine
            .create_task(CreateTask {
                title: "Task A".into(),
                description: "First".into(),
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
                description: "Second".into(),
                epic_id: None,
                acceptance_criteria: vec![],
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        // Clear and rebuild
        engine.rebuild_projections().await.unwrap();

        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        assert_eq!(backlog.task_count(), 2);
    }

    #[tokio::test]
    async fn full_task_lifecycle() {
        let engine = RewindEngine::in_memory().await;

        let events = engine
            .create_task(CreateTask {
                title: "Lifecycle test".into(),
                description: "".into(),
                epic_id: None,
                acceptance_criteria: vec![],
                story_type: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        let task_id = match &events[0] {
            RewindEvent::TaskCreated { task_id, .. } => task_id.clone(),
            _ => panic!("Expected TaskCreated"),
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
                task_id: task_id.clone(),
                session_id: SessionId::generate(),
                discretionary_note: None,
            })
            .await
            .unwrap();

        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        let task = backlog.tasks.get(task_id.as_ref()).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn session_lifecycle() {
        let engine = RewindEngine::in_memory().await;

        let events = engine.start_session().await.unwrap();
        assert_eq!(events.len(), 1);
        matches!(&events[0], RewindEvent::SessionStarted { .. });
    }
}
