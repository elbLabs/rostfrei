#![allow(dead_code)]

use domain::DecisionOutcome;
use domain::{
    Aggregate, BoundedContext, DecisionGroupType, DomainIdentity, Entity, domain_decisions,
};

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
    assert_eq!(FirstDecisions::DECISIONS.len(), 1);
    assert_eq!(SecondDecisions::DECISIONS.len(), 1);
    let _ = (Owner::first(), Owner::second());
}
