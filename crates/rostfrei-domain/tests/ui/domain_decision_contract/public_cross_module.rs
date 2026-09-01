#![allow(dead_code)]

use domain::domain_decision_test;

mod owner {
    use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions};
    use domain::DecisionOutcome;

    pub struct PublicDecisions;

    #[derive(BoundedContext)]
    #[domain(id = "context", label = "Context")]
    pub struct Context;

    #[derive(DomainIdentity)]
    #[domain(owner = Root)]
    pub struct RootId(u8);

    #[derive(Entity)]
#[domain(id = "root", label = "Root")]
pub struct Root {
        #[domain(identity)]
        id: RootId,
    }

    #[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
pub struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = RootId;
}

    impl domain::AttachedDecisionGroup<PublicDecisions> for Owner {}

    #[derive(DecisionOutcome, Debug, Eq, PartialEq)]
    pub enum Outcome {
        #[outcome(id = "checked", label = "Checked")]
        Checked(u8),
    }

    #[domain_decisions(aggregate, group = PublicDecisions)]
    impl Owner {
        #[decision(id = "check", label = "Check")]
        pub fn check(value: u8) -> Outcome {
            Outcome::Checked(value)
        }
    }
}

#[domain_decision_test(owner::Owner::CHECK)]
fn public_decision_can_be_tested_from_another_module() {
    assert_eq!(owner::Owner::check(1), owner::Outcome::Checked(1));
}

fn main() {}
