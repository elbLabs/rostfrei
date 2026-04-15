#![allow(dead_code)]

use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, ValueObject, domain_decisions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner, decisions)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

#[derive(ValueObject)]
#[domain(id = "output", label = "Output", owner = Owner)]
struct Output(u8);

#[domain_decisions(entity)]
impl Root {
    #[decision(id = "decide", label = "Decide")]
    fn decide(value: u8) -> Result<Output, Output> {
        Ok(Output(value))
    }
}

fn main() {
    let Ok(output) = Root::decide(1) else {
        panic!("decision should succeed");
    };
    assert_eq!(output.0, 1);
}
