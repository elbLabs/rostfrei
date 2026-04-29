use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions};
use domain::DecisionOutcome;

struct Decisions;

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
struct RootId(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: RootId,
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = RootId;
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

#[derive(DecisionOutcome)]
enum Outcome {
    #[outcome(id = "done", label = "Done")]
    Done,
}

#[domain_decisions(entity, group = Decisions)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide() -> Outcome {
        Outcome::Done
    }
}

fn main() {}
