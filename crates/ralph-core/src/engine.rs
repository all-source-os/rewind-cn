use std::path::Path;
use std::sync::Arc;

use allframe::cqrs::allsource_backend::AllSourceBackend;
use allframe::cqrs::{CommandBus, EventStore, InMemoryBackend, Projection};
use tokio::sync::RwLock;
use tracing::info;

use crate::domain::commands::*;
use crate::domain::events::RalphEvent;
use crate::domain::projections::{BacklogProjection, EpicProgressProjection};

/// Central engine that wires together EventStore, CommandBus, and Projections.
pub struct RalphEngine<B: allframe::cqrs::EventStoreBackend<RalphEvent> = AllSourceBackend<RalphEvent>>
{
    pub event_store: Arc<EventStore<RalphEvent, B>>,
    pub command_bus: CommandBus<RalphEvent>,
    backlog: Arc<RwLock<BacklogProjection>>,
    epic_progress: Arc<RwLock<EpicProgressProjection>>,
}

impl RalphEngine<AllSourceBackend<RalphEvent>> {
    /// Initialize a new engine with persistent storage at the given path.
    pub async fn init(data_path: &str) -> Result<Self, String> {
        let path = Path::new(data_path);
        if !path.exists() {
            std::fs::create_dir_all(path)
                .map_err(|e| format!("Failed to create data directory: {e}"))?;
        }

        let backend = AllSourceBackend::production(data_path)?;
        let event_store = Arc::new(EventStore::with_backend(backend));
        let command_bus = CommandBus::new();

        let mut engine = Self {
            event_store,
            command_bus,
            backlog: Arc::new(RwLock::new(BacklogProjection::default())),
            epic_progress: Arc::new(RwLock::new(EpicProgressProjection::default())),
        };
        engine.register_handlers().await;

        info!("Ralph engine initialized at {data_path}");
        Ok(engine)
    }

    /// Load an existing engine from a data path.
    pub async fn load(data_path: &str) -> Result<Self, String> {
        if !Path::new(data_path).exists() {
            return Err(format!(
                "Data directory does not exist: {data_path}. Run `ralph init` first."
            ));
        }
        Self::init(data_path).await
    }
}

impl RalphEngine<InMemoryBackend<RalphEvent>> {
    /// Create an in-memory engine for testing.
    pub async fn in_memory() -> Self {
        let event_store = Arc::new(EventStore::new());
        let command_bus = CommandBus::new();

        let mut engine = Self {
            event_store,
            command_bus,
            backlog: Arc::new(RwLock::new(BacklogProjection::default())),
            epic_progress: Arc::new(RwLock::new(EpicProgressProjection::default())),
        };
        engine.register_handlers().await;
        engine
    }
}

impl<B: allframe::cqrs::EventStoreBackend<RalphEvent>> RalphEngine<B> {
    async fn register_handlers(&mut self) {
        self.command_bus
            .register::<CreateTask, _>(CreateTaskHandler)
            .await;
        self.command_bus
            .register::<AssignTask, _>(AssignTaskHandler)
            .await;
        self.command_bus
            .register::<CompleteTask, _>(CompleteTaskHandler)
            .await;
        self.command_bus
            .register::<FailTask, _>(FailTaskHandler)
            .await;
        self.command_bus
            .register::<CreateEpic, _>(CreateEpicHandler)
            .await;
        self.command_bus
            .register::<CompleteEpic, _>(CompleteEpicHandler)
            .await;
    }

    /// Apply an event to all projections.
    async fn apply_to_projections(&self, event: &RalphEvent) {
        self.backlog.write().await.apply(event);
        self.epic_progress.write().await.apply(event);
    }

    /// Get a read handle to the backlog projection.
    pub fn backlog(&self) -> Arc<RwLock<BacklogProjection>> {
        self.backlog.clone()
    }

    /// Get a read handle to the epic progress projection.
    pub fn epic_progress(&self) -> Arc<RwLock<EpicProgressProjection>> {
        self.epic_progress.clone()
    }

    /// Dispatch a command, append resulting events, and update projections.
    pub async fn dispatch_and_append<C: allframe::cqrs::Command>(
        &self,
        aggregate_id: &str,
        command: C,
    ) -> Result<Vec<RalphEvent>, allframe::cqrs::CommandError> {
        let events = self.command_bus.dispatch::<C>(command).await?;
        self.event_store
            .append(aggregate_id, events.clone())
            .await
            .map_err(|e| allframe::cqrs::CommandError::Internal(e))?;
        for event in &events {
            self.apply_to_projections(event).await;
        }
        Ok(events)
    }

    /// Rebuild all projections from the event store.
    pub async fn rebuild_projections(&self) -> Result<(), String> {
        let events = self.event_store.get_all_events().await?;

        let mut backlog = self.backlog.write().await;
        let mut epic_progress = self.epic_progress.write().await;
        *backlog = BacklogProjection::default();
        *epic_progress = EpicProgressProjection::default();

        for event in &events {
            backlog.apply(event);
            epic_progress.apply(event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::commands::CreateTask;

    #[tokio::test]
    async fn in_memory_engine_roundtrip() {
        let engine = RalphEngine::in_memory().await;

        let events = engine
            .dispatch_and_append(
                "task-1",
                CreateTask {
                    title: "Write tests".into(),
                    description: "Add unit tests".into(),
                    epic_id: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(events.len(), 1);

        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        assert_eq!(backlog.task_count(), 1);
    }

    #[tokio::test]
    async fn rebuild_projections_replays_events() {
        let engine = RalphEngine::in_memory().await;

        engine
            .dispatch_and_append(
                "task-1",
                CreateTask {
                    title: "Task A".into(),
                    description: "First".into(),
                    epic_id: None,
                },
            )
            .await
            .unwrap();

        engine
            .dispatch_and_append(
                "task-2",
                CreateTask {
                    title: "Task B".into(),
                    description: "Second".into(),
                    epic_id: None,
                },
            )
            .await
            .unwrap();

        // Clear and rebuild
        engine.rebuild_projections().await.unwrap();

        let backlog = engine.backlog();
        let backlog = backlog.read().await;
        assert_eq!(backlog.task_count(), 2);
    }
}
