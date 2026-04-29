#![allow(dead_code)]

use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, ValueObject, domain_decisions};
use domain::DecisionOutcome;

struct RootDecisions;

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

#[derive(ValueObject, Debug, Eq, PartialEq)]
#[domain(id = "output", label = "Output")]
struct Output(u8);

#[derive(DecisionOutcome, Debug, Eq, PartialEq)]
enum Outcome {
    #[outcome(id = "accepted", label = "Accepted")]
    Accepted(Output),
}

#[domain_decisions(entity, group = RootDecisions)]
impl Root {
    #[decision(id = "decide", label = "Decide")]
    fn decide(value: u8) -> Outcome {
        Outcome::Accepted(Output(value))
    }
}

fn main() {
    assert_eq!(Root::decide(1), Outcome::Accepted(Output(1)));
}
