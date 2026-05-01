use rostfrei::{AggregateInstance, Apply, CommandHandler, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "context", label = "Context")]
struct Context;

#[derive(rostfrei::DomainIdentity)]
struct Id(u64);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "root", label = "Root")]
struct Root {
    id: Id,
}

impl rostfrei::EntityDefinition for Root {
    type Owner = Aggregate;
    type Identity = Id;

    fn identity(&self) -> &Self::Identity { &self.id }
}

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "registered", label = "Registered")]
struct Registered;

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "unregistered", label = "Unregistered")]
struct Unregistered;

#[derive(rostfrei::AggregateEvents)]
enum Events {
    Registered(Registered),
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

impl Apply<Registered> for Root {
    fn apply(&mut self, _: &Registered) {}
}

struct Command;

impl CommandHandler<Command> for Aggregate {
    type Rejection = ();

    fn handle(_: &Command, aggregate: &mut AggregateInstance<Self>) -> Result<(), ()> {
        aggregate.raise(Unregistered);
        Ok(())
    }
}

fn main() {}
rostfrei::install_macro_support!();
