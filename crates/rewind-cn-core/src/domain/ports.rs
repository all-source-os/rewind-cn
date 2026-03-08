use async_trait::async_trait;

use super::error::RewindError;
use super::events::RewindEvent;

/// Port for persisting and retrieving events.
#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn append(&self, aggregate_id: &str, events: Vec<RewindEvent>)
        -> Result<(), RewindError>;
    async fn get_all_events(&self) -> Result<Vec<RewindEvent>, RewindError>;
}

/// Port for dispatching commands and getting resulting events.
#[async_trait]
pub trait CommandDispatcher: Send + Sync {
    async fn dispatch_and_append(
        &self,
        aggregate_id: &str,
        events: Vec<RewindEvent>,
    ) -> Result<Vec<RewindEvent>, RewindError>;
}
