#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize, rostfrei::ReadModel)]
#[read_model(id = "access-view", version = 1)]
pub struct View { pub enabled: bool }

#[derive(rostfrei::BoundedContext)]
#[domain(id = "access", label = "Access")]
pub struct Access;

#[derive(Deserialize, Serialize, rostfrei::Command)]
#[domain(context = Access, id = "change-access", label = "Change access")]
pub struct ChangeAccess;

#[derive(Deserialize, Serialize, rostfrei::QueryDefinition)]
#[rostfrei(context = "access", name = "get-access", version = 1, response = bool)]
pub struct GetAccess;

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "changed", label = "Changed")]
pub struct Changed { pub id: String }

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "foreign", label = "Foreign")]
pub struct ForeignEvent;

#[derive(rostfrei::AggregateEvents)]
pub enum Events { Changed(Changed) }

pub struct Organization;
impl rostfrei::Aggregate for Organization {
    type State = ();
    type Event = Events;
    const BOUNDED_CONTEXT: &'static str = "access";
    const AGGREGATE_TYPE: &'static str = "organization";
    fn initial(_: &rostfrei::StreamId) {}
    fn apply(_: &mut (), _: &Events) {}
}

#[derive(Deserialize, Serialize)]
pub struct PublicChanged { pub id: String, pub version: u64 }
impl rostfrei::IntegrationEvent for PublicChanged {
    const EVENT_NAME: &'static str = "public-changed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("billing");
}
