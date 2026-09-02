#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions,
};
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

struct Input(u8);

#[derive(DecisionOutcome)]
enum Outcome {
    #[outcome(id = "done", label = "Done")]
    Done,
}

#[domain_decisions(aggregate, group = Decisions)]
impl Owner {
    #[decision(id = "owned", label = "Owned")]
    fn owned(input: Input) -> Outcome {
        let _ = input;
        Outcome::Done
    }

    #[decision(id = "borrowed", label = "Borrowed")]
    fn borrowed(input: &Input) -> Outcome {
        let _ = input;
        Outcome::Done
    }
}

fn main() {
    let input = Input(1);
    let _ = Owner::owned(Input(1));
    let _ = Owner::borrowed(&input);
}
