#![allow(dead_code)]

use domain::{
    Aggregate, AggregateType, BoundedContext, DomainIdentity, Entity, ValueObject, domain_decisions,
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
#[domain(
    id = "owner",
    label = "Owner",
    context = Context,
    root = Root,
    decisions = [Decisions]
)]
struct Owner;

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
        Owner::DECISION_GROUPS[0][0].parameters,
        Owner::DECISION_GROUPS[0][1].parameters
    );
    let input = Input(1);
    let _ = Owner::owned(Input(1));
    let _ = Owner::borrowed(&input);
}
