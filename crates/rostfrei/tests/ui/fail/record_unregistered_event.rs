use async_trait::async_trait;
use rostfrei::{
    Apply, CommandDecision, CommandHandler, CommandExecution,
    CommandHandlingResult, Initialize,
};
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
struct Handler;

#[async_trait]
impl CommandHandler<Command> for Handler {
    type Rejection = ();

    async fn handle(
        &self,
        _: &Command,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut aggregate = execution.load::<Aggregate>("aggregate").await?;
        aggregate.aggregate_mut().raise(Unregistered);
        Ok(CommandDecision::Accepted)
    }
}

fn main() {}
rostfrei::install_macro_support!();
