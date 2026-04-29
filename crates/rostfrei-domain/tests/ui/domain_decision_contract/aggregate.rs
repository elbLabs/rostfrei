#![allow(dead_code)]

use domain::DecisionOutcome;
use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, ValueObject, domain_decisions};

struct OwnerDecisions;

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
    Accepted(Output, bool),
    #[outcome(id = "rejected", label = "Rejected")]
    Rejected { code: u8, retryable: bool },
}

#[domain_decisions(aggregate, group = OwnerDecisions)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide(value: u8, accepted: bool) -> Outcome {
        if accepted {
            Outcome::Accepted(Output(value), true)
        } else {
            Outcome::Rejected {
                code: value,
                retryable: false,
            }
        }
    }
}

fn main() {
    assert_eq!(Owner::decide(1, true), Outcome::Accepted(Output(1), true));
}
