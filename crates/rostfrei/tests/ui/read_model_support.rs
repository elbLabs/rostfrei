#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize, rostfrei::ReadModel)]
#[read_model(id = "fleet-status", version = 1)]
pub struct View { pub enabled: bool }

#[derive(rostfrei::BoundedContext)]
#[domain(id = "bike-rental", label = "BikeRental")]
pub struct BikeRental;

#[derive(Deserialize, Serialize, rostfrei::Command)]
#[domain(context = BikeRental, id = "reserve-cycle", label = "Reserve cycle")]
pub struct ReserveCycle;

#[derive(Deserialize, Serialize, rostfrei::QueryDefinition)]
#[rostfrei(context = "bike-rental", name = "find-cycle", version = 1, response = bool)]
pub struct FindCycle;

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "changed", label = "Changed")]
pub struct Changed { pub id: String }

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "foreign", label = "Foreign")]
pub struct ForeignEvent;

#[derive(rostfrei::AggregateEvents)]
pub enum Events { Changed(Changed) }

pub struct RentalFleet;
impl rostfrei::Aggregate for RentalFleet {
    type State = ();
    type Event = Events;
    const BOUNDED_CONTEXT: &'static str = "bike-rental";
    const AGGREGATE_TYPE: &'static str = "rental-fleet";
    fn initial(_: &rostfrei::StreamId) {}
    fn apply(_: &mut (), _: &Events) {}
}

#[derive(Deserialize, Serialize)]
pub struct PublicChanged { pub id: String, pub version: u64 }
impl rostfrei::IntegrationEvent for PublicChanged {
    const EVENT_NAME: &'static str = "public-changed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("workshop");
}
