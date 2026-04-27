#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DecisionGroupType, DomainIdentity, Entity, ValueObject,
    domain_decisions,
};
use domain::DecisionOutcome;

struct Decisions;

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

#[derive(ValueObject)]
#[domain(id = "input", label = "Input", owner = Owner)]
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
    assert_eq!(
        Decisions::DECISIONS[0].parameters,
        Decisions::DECISIONS[1].parameters
    );
    let input = Input(1);
    let _ = Owner::owned(Input(1));
    let _ = Owner::borrowed(&input);
}
