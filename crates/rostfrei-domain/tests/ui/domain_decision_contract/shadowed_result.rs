#![allow(dead_code)]

use domain::DecisionOutcome;
use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions};

struct Decisions;

#[derive(DecisionOutcome)]
enum Result {
    #[outcome(id = "done", label = "Done")]
    Done,
}

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

#[domain_decisions(aggregate, group = Decisions)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide() -> Result {
        Result::Done
    }
}

fn main() {
    let _ = Owner::decide();
}
