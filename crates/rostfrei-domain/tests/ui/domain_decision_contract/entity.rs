#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, ValueObject, domain_decisions,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "input", label = "Input", owner = Context)]
struct Input(u8);

#[derive(ValueObject)]
#[domain(id = "output", label = "Output", owner = Context)]
struct Output(u8);

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", label = "Decide")]
    fn decide(input: Input) -> Output;
}

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u8);

#[derive(Entity)]
#[domain(
    id = "root",
    label = "Root",
    owner = Owner,
    decisions = [Decisions]
)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

impl Decisions for Root {
    fn decide(input: Input) -> Output {
        Output(input.0)
    }
}

fn main() {
    let output = <Root as Decisions>::decide(Input(1));
    assert_eq!(output.0, 1);
}
