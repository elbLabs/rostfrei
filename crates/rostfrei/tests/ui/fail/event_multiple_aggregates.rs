use rostfrei::{Apply, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "context", label = "Context")]
struct Context;

#[derive(rostfrei::DomainIdentity)]
struct FirstId(u64);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "first", label = "First")]
struct FirstRoot {
    id: FirstId,
}

impl rostfrei::EntityDefinition for FirstRoot {
    type Owner = FirstAggregate;
    type Identity = FirstId;

    fn identity(&self) -> &Self::Identity { &self.id }
}

#[derive(rostfrei::DomainIdentity)]
struct SecondId(u64);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "second", label = "Second")]
struct SecondRoot {
    id: SecondId,
}

impl rostfrei::EntityDefinition for SecondRoot {
    type Owner = SecondAggregate;
    type Identity = SecondId;

    fn identity(&self) -> &Self::Identity { &self.id }
}

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "shared", label = "Shared")]
struct SharedEvent;

#[derive(rostfrei::AggregateEvents)]
enum FirstEvents {
    Shared(SharedEvent),
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(id = "first", label = "First")]
struct FirstAggregate;

impl rostfrei::AggregateDefinition for FirstAggregate {
    type Context = Context;
    type Root = FirstRoot;
    type Event = FirstEvents;
}

#[derive(rostfrei::AggregateEvents)]
enum SecondEvents {
    Shared(SharedEvent),
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(id = "second", label = "Second")]
struct SecondAggregate;

impl rostfrei::AggregateDefinition for SecondAggregate {
    type Context = Context;
    type Root = SecondRoot;
    type Event = SecondEvents;
}

impl Initialize<FirstAggregate> for FirstRoot {
    fn initialize(_: &rostfrei::StreamId) -> Self { Self { id: FirstId(1) } }
}

impl Initialize<SecondAggregate> for SecondRoot {
    fn initialize(_: &rostfrei::StreamId) -> Self { Self { id: SecondId(2) } }
}

impl Apply<SharedEvent> for FirstRoot {
    fn apply(&mut self, _: &SharedEvent) {}
}

impl Apply<SharedEvent> for SecondRoot {
    fn apply(&mut self, _: &SharedEvent) {}
}

fn main() {}
rostfrei::install_macro_support!();
