#![allow(dead_code)]

use domain::DecisionOutcome;
use domain::{Aggregate, AggregateType, BoundedContext, DomainIdentity, Entity, domain_decisions};

struct FirstDecisions;
struct SecondDecisions;

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
    decisions = [FirstDecisions, SecondDecisions]
)]
struct Owner;

#[derive(DecisionOutcome)]
enum Outcome {
    #[outcome(id = "done", label = "Done")]
    Done,
}

#[domain_decisions(aggregate, group = FirstDecisions)]
impl Owner {
    #[decision(id = "first", label = "First")]
    fn first() -> Outcome {
        Outcome::Done
    }
}

#[domain_decisions(aggregate, group = SecondDecisions)]
impl Owner {
    #[decision(id = "second", label = "Second")]
    fn second() -> Outcome {
        Outcome::Done
    }
}

fn main() {
    assert_eq!(Owner::DECISION_GROUPS.len(), 2);
    let _ = (Owner::first(), Owner::second());
}
