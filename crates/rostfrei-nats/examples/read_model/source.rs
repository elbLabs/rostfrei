//! Small authoritative source model used by the example and integration tests.

use rostfrei_core::{Aggregate, StreamId};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "member-joined", label = "Member joined")]
pub struct MemberJoined {
    pub organization_id: String,
}

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "demo-changed", label = "Demo changed")]
pub struct DemoChanged {
    pub organization_id: String,
    pub active: bool,
}

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "organization-renamed", label = "Organization renamed")]
pub struct OrganizationRenamed {
    pub name: String,
}

#[derive(rostfrei::AggregateEvents)]
pub enum OrganizationEvent {
    MemberJoined(MemberJoined),
    DemoChanged(DemoChanged),
    OrganizationRenamed(OrganizationRenamed),
}

pub struct Organization;

impl Aggregate for Organization {
    type State = (u64, bool);
    type Event = OrganizationEvent;
    const BOUNDED_CONTEXT: &'static str = "access";
    const AGGREGATE_TYPE: &'static str = "organization";
    fn initial(_: &StreamId) -> Self::State {
        (0, false)
    }
    fn apply(state: &mut Self::State, event: &Self::Event) {
        match event {
            OrganizationEvent::MemberJoined(_) => state.0 = state.0.saturating_add(1),
            OrganizationEvent::DemoChanged(event) => state.1 = event.active,
            OrganizationEvent::OrganizationRenamed(_) => {}
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct BillingChanged {
    pub organization_id: String,
    pub paid: bool,
}

impl rostfrei::IntegrationEvent for BillingChanged {
    const EVENT_NAME: &'static str = "billing-changed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("billing");
}
