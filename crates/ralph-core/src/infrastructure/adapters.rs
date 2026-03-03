use allframe::cqrs::{Aggregate, Projection};

use crate::domain::events::RalphEvent;
use crate::domain::model::{
    BacklogProjection, EpicAggregate, EpicProgressProjection, TaskAggregate,
};

impl Aggregate for TaskAggregate {
    type Event = RalphEvent;

    fn apply_event(&mut self, event: &RalphEvent) {
        // Delegate to the domain method (same name, different trait)
        Self::apply_event(self, event);
    }
}

impl Aggregate for EpicAggregate {
    type Event = RalphEvent;

    fn apply_event(&mut self, event: &RalphEvent) {
        Self::apply_event(self, event);
    }
}

impl Projection for BacklogProjection {
    type Event = RalphEvent;

    fn apply(&mut self, event: &RalphEvent) {
        self.apply_event(event);
    }
}

impl Projection for EpicProgressProjection {
    type Event = RalphEvent;

    fn apply(&mut self, event: &RalphEvent) {
        self.apply_event(event);
    }
}
