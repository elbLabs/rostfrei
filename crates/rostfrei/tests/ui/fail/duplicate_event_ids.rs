use rostfrei::{Apply, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "context", label = "Context")]
struct Context;

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = Root)]
struct Id(u64);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "root", label = "Root", owner = Aggregate)]
struct Root {
    #[rostfrei(identity)]
    id: Id,
}

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "duplicate", label = "First")]
struct FirstEvent;

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "duplicate", label = "Second")]
struct SecondEvent;

#[derive(rostfrei::AggregateEvents)]
enum Events {
    First(FirstEvent),
    Second(SecondEvent),
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(id = "aggregate", label = "Aggregate")]
struct Aggregate;

impl rostfrei::AggregateDefinition for Aggregate {
    type Context = Context;
    type Root = Root;
    type Event = Events;
}

impl Initialize<Aggregate> for Root {
    fn initialize(_: &rostfrei::StreamId) -> Self { Self { id: Id(1) } }
}

impl Apply<FirstEvent> for Root {
    fn apply(&mut self, _: &FirstEvent) {}
}

impl Apply<SecondEvent> for Root {
    fn apply(&mut self, _: &SecondEvent) {}
}

fn main() {}
