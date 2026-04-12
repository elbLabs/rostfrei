use rostfrei::{Apply, CommandHandler, DecisionContext, Initialize};
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
#[rostfrei(id = "registered", label = "Registered")]
struct Registered;

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "unregistered", label = "Unregistered")]
struct Unregistered;

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "aggregate",
    label = "Aggregate",
    context = Context,
    root = Root,
    events = [Registered]
)]
struct Aggregate;

impl Initialize<Aggregate> for Root {
    fn initialize(_: &rostfrei::StreamId) -> Self { Self { id: Id(1) } }
}

impl Apply<Registered> for Root {
    fn apply(&mut self, _: &Registered) {}
}

struct Command;

impl CommandHandler<Command> for Aggregate {
    type Rejection = ();

    fn handle(_: &Command, context: &mut DecisionContext<'_, Self>) -> Result<(), ()> {
        context.record(Unregistered);
        Ok(())
    }
}

fn main() {}
