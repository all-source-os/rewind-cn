use std::path::Path;
use std::sync::Arc;

use allframe::cqrs::allsource_backend::AllSourceBackend;
use allframe::cqrs::{CommandBus, EventStore, InMemoryBackend, Projection};
use tokio::sync::RwLock;
use tracing::info;

use crate::application::commands;
use crate::domain::error::RalphError;
use crate::domain::events::RalphEvent;
use crate::domain::model::{BacklogProjection, EpicProgressProjection};

use super::command_bridge::*;

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
    pub async fn init(data_path: &str) -> Result<Self, RalphError> {
        let path = Path::new(data_path);
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(|e| {
                RalphError::Storage(format!("Failed to create data directory: {e}"))
            })?;
        }

        let backend =
            AllSourceBackend::production(data_path).map_err(|e| RalphError::Storage(e))?;
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
    pub async fn load(data_path: &str) -> Result<Self, RalphError> {
        if !Path::new(data_path).exists() {
            return Err(RalphError::NotFound(format!(
                "Data directory does not exist: {data_path}. Run `ralph init` first."
            )));
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

    pub async fn create_task(
        &self,
        cmd: commands::CreateTask,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        self.dispatch_and_append("task", CreateTaskCmd(cmd)).await
    }

    pub async fn assign_task(
        &self,
        cmd: commands::AssignTask,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, AssignTaskCmd(cmd)).await
    }

    pub async fn start_task(
        &self,
        cmd: commands::StartTask,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, StartTaskCmd(cmd)).await
    }

    pub async fn complete_task(
        &self,
        cmd: commands::CompleteTask,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, CompleteTaskCmd(cmd))
            .await
    }

    pub async fn fail_task(
        &self,
        cmd: commands::FailTask,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        let agg_id = cmd.task_id.to_string();
        self.dispatch_and_append(&agg_id, FailTaskCmd(cmd)).await
    }

    pub async fn create_epic(
        &self,
        cmd: commands::CreateEpic,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        self.dispatch_and_append("epic", CreateEpicCmd(cmd)).await
    }

    pub async fn complete_epic(
        &self,
        cmd: commands::CompleteEpic,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        let agg_id = cmd.epic_id.to_string();
        self.dispatch_and_append(&agg_id, CompleteEpicCmd(cmd))
            .await
    }

    pub async fn start_session(&self) -> Result<Vec<RalphEvent>, RalphError> {
        self.dispatch_and_append("session", StartSessionCmd(commands::StartSession))
            .await
    }

    pub async fn end_session(
        &self,
        cmd: commands::EndSession,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        let agg_id = cmd.session_id.to_string();
        self.dispatch_and_append(&agg_id, EndSessionCmd(cmd)).await
    }

    /// Generic dispatch: dispatches a command, appends resulting events, and updates projections.
    async fn dispatch_and_append<C: allframe::cqrs::Command>(
        &self,
        aggregate_id: &str,
        command: C,
    ) -> Result<Vec<RalphEvent>, RalphError> {
        let events = self
            .command_bus
            .dispatch::<C>(command)
            .await
            .map_err(|e| RalphError::InvalidState(e.to_string()))?;
        self.event_store
            .append(aggregate_id, events.clone())
            .await
            .map_err(|e| RalphError::Storage(e))?;
        for event in &events {
            self.apply_to_projections(event).await;
        }
        Ok(events)
    }

    /// Rebuild all projections from the event store.
    pub async fn rebuild_projections(&self) -> Result<(), RalphError> {
        let events = self
            .event_store
            .get_all_events()
            .await
            .map_err(|e| RalphError::Storage(e))?;

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
    use crate::application::commands::{
        AssignTask, CompleteTask, CreateTask, StartTask,
    };
    use crate::domain::ids::AgentId;
    use crate::domain::model::TaskStatus;

    #[tokio::test]
    async fn in_memory_engine_roundtrip() {
        let engine = RalphEngine::in_memory().await;

        let events = engine
            .create_task(CreateTask {
                title: "Write tests".into(),
                description: "Add unit tests".into(),
                epic_id: None,
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
        let engine = RalphEngine::in_memory().await;

        engine
            .create_task(CreateTask {
                title: "Task A".into(),
                description: "First".into(),
                epic_id: None,
            })
            .await
            .unwrap();

        engine
            .create_task(CreateTask {
                title: "Task B".into(),
                description: "Second".into(),
                epic_id: None,
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
        let engine = RalphEngine::in_memory().await;

        let events = engine
            .create_task(CreateTask {
                title: "Lifecycle test".into(),
                description: "".into(),
                epic_id: None,
            })
            .await
            .unwrap();

        let task_id = match &events[0] {
            RalphEvent::TaskCreated { task_id, .. } => task_id.clone(),
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
        let engine = RalphEngine::in_memory().await;

        let events = engine.start_session().await.unwrap();
        assert_eq!(events.len(), 1);
        matches!(&events[0], RalphEvent::SessionStarted { .. });
    }
}
