use rostfrei::{Apply, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "context", label = "Context")]
struct Context;

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = Root)]
struct Id(u64);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "root", label = "Root")]
struct Root {
    #[rostfrei(identity)]
    id: Id,
}

impl rostfrei::EntityDefinition for Root {
    type Owner = Aggregate;
    type Identity = Id;
}

struct WrongRoot;

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "event", label = "Event")]
struct Event;

#[derive(rostfrei::AggregateEvents)]
enum Events {
    Event(Event),
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

impl Apply<Event> for WrongRoot {
    fn apply(&mut self, _: &Event) {}
}

fn main() {}
