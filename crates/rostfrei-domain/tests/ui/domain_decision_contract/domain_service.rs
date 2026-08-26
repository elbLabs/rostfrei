#![allow(dead_code)]

use domain::{BoundedContext, DomainService, ValueObject, domain_decisions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "input", label = "Input", owner = Context)]
struct Input(u8);

#[derive(ValueObject)]
#[domain(id = "output", label = "Output", owner = Context)]
struct Output(u8);

#[domain_decisions(domain_service)]
trait Decisions {
    #[decision(id = "decide", label = "Decide")]
    fn decide(input: Input) -> Output;
}

#[derive(DomainService)]
#[domain(
    id = "service",
    label = "Service",
    context = Context,
    decisions = [Decisions]
)]
struct Service;

impl Decisions for Service {
    fn decide(input: Input) -> Output {
        Output(input.0)
    }
}

fn main() {
    let output = <Service as Decisions>::decide(Input(1));
    assert_eq!(output.0, 1);
}
