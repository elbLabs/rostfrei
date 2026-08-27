#![allow(dead_code)]

use domain::domain_decision_test;

mod owner {
    use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions};

    #[derive(BoundedContext)]
    #[domain(id = "context", label = "Context")]
    pub struct Context;

    #[derive(DomainIdentity)]
    #[domain(owner = Root)]
    pub struct RootId(u8);

    #[derive(Entity)]
    #[domain(id = "root", label = "Root", owner = Owner)]
    pub struct Root {
        #[domain(identity)]
        id: RootId,
    }

    #[derive(Aggregate)]
    #[domain(id = "owner", label = "Owner", context = Context, root = Root, decisions)]
    pub struct Owner;

    #[domain_decisions(aggregate)]
    impl Owner {
        #[decision(id = "check", label = "Check")]
        pub fn check(value: u8) -> Result<u8, u8> {
            Ok(value)
        }
    }
}

#[domain_decision_test(owner::Owner::CHECK)]
fn public_decision_can_be_tested_from_another_module() {
    assert_eq!(owner::Owner::check(1), Ok(1));
}

fn main() {}
