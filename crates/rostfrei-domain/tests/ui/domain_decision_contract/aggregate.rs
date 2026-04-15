#![allow(dead_code)]

use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, ValueObject, domain_decisions};

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
#[domain(id = "owner", label = "Owner", context = Context, root = Root, decisions)]
struct Owner;

#[derive(ValueObject)]
#[domain(id = "output", label = "Output", owner = Owner)]
struct Output(u8);

#[domain_decisions(aggregate)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide(value: u8, accepted: bool) -> Result<Output, Output> {
        accepted.then_some(Output(value)).ok_or(Output(value))
    }
}

fn main() {
    let Ok(output) = Owner::decide(1, true) else {
        panic!("decision should succeed");
    };
    assert_eq!(output.0, 1);
}
