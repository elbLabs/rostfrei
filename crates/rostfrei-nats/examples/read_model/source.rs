//! Small authoritative source model used by the example and integration tests.

use rostfrei_core::{Aggregate, StreamId};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, rostfrei_runtime::DomainEvent)]
#[domain(id = "bicycle-registered", label = "Bicycle registered")]
pub struct BicycleRegistered {
    pub fleet_id: String,
}

#[derive(Deserialize, Serialize, rostfrei_runtime::DomainEvent)]
#[domain(id = "rental-window-changed", label = "Rental window changed")]
pub struct RentalWindowChanged {
    pub fleet_id: String,
    pub open: bool,
}

#[derive(Deserialize, Serialize, rostfrei_runtime::DomainEvent)]
#[domain(id = "fleet-renamed", label = "Fleet renamed")]
pub struct FleetRenamed {
    pub name: String,
}

#[derive(rostfrei_runtime::AggregateEvents)]
pub enum FleetEvent {
    BicycleRegistered(BicycleRegistered),
    RentalWindowChanged(RentalWindowChanged),
    FleetRenamed(FleetRenamed),
}

pub struct Fleet;

impl Aggregate for Fleet {
    type State = (u64, bool);
    type Event = FleetEvent;
    const BOUNDED_CONTEXT: &'static str = "bike-rental";
    const AGGREGATE_TYPE: &'static str = "fleet";
    fn initial(_: &StreamId) -> Self::State {
        (0, false)
    }
    fn apply(state: &mut Self::State, event: &Self::Event) {
        match event {
            FleetEvent::BicycleRegistered(_) => state.0 = state.0.saturating_add(1),
            FleetEvent::RentalWindowChanged(event) => state.1 = event.open,
            FleetEvent::FleetRenamed(_) => {}
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct InspectionChanged {
    pub fleet_id: String,
    pub inspection_passed: bool,
}

impl rostfrei_runtime::IntegrationEvent for InspectionChanged {
    const EVENT_NAME: &'static str = "inspection-changed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("workshop");
}
