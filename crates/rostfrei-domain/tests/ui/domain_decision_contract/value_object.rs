#![allow(dead_code)]

use rostfrei_domain::{BoundedContext, ValueObject, domain_decisions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "output", label = "Output", owner = Context)]
struct Output(u8);

#[domain_decisions(value_object)]
trait Decisions {
    #[decision(id = "decide", label = "Decide")]
    fn decide(input: Input) -> Output;
}

#[derive(ValueObject)]
#[domain(
    id = "input",
    label = "Input",
    owner = Context,
    decisions = [Decisions]
)]
struct Input(u8);

impl Decisions for Input {
    fn decide(input: Input) -> Output {
        Output(input.0)
    }
}

fn main() {
    let output = <Input as Decisions>::decide(Input(1));
    assert_eq!(output.0, 1);
}
