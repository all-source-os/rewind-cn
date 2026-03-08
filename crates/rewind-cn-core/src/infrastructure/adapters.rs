use allframe::cqrs::{Aggregate, Projection};

use crate::domain::events::RewindEvent;
use crate::domain::model::{
    BacklogProjection, EpicAggregate, EpicProgressProjection, TaskAggregate,
};

impl Aggregate for TaskAggregate {
    type Event = RewindEvent;

    fn apply_event(&mut self, event: &RewindEvent) {
        // Delegate to the domain method (same name, different trait)
        Self::apply_event(self, event);
    }
}

impl Aggregate for EpicAggregate {
    type Event = RewindEvent;

    fn apply_event(&mut self, event: &RewindEvent) {
        Self::apply_event(self, event);
    }
}

impl Projection for BacklogProjection {
    type Event = RewindEvent;

    fn apply(&mut self, event: &RewindEvent) {
        self.apply_event(event);
    }
}

impl Projection for EpicProgressProjection {
    type Event = RewindEvent;

    fn apply(&mut self, event: &RewindEvent) {
        self.apply_event(event);
    }
}
