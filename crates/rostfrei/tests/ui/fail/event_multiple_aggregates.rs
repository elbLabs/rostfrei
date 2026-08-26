use rostfrei::{Apply, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "context", label = "Context")]
struct Context;

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = FirstRoot)]
struct FirstId(u64);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "first", label = "First", owner = FirstAggregate)]
struct FirstRoot {
    #[rostfrei(identity)]
    id: FirstId,
}

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = SecondRoot)]
struct SecondId(u64);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "second", label = "Second", owner = SecondAggregate)]
struct SecondRoot {
    #[rostfrei(identity)]
    id: SecondId,
}

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "shared", label = "Shared")]
struct SharedEvent;

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "first",
    label = "First",
    context = Context,
    root = FirstRoot,
    events = [SharedEvent]
)]
struct FirstAggregate;

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "second",
    label = "Second",
    context = Context,
    root = SecondRoot,
    events = [SharedEvent]
)]
struct SecondAggregate;

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
