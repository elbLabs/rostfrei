use domain::{BoundedContext, ValueObject, domain_decisions};

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
    fn decide(input: Input) -> Vec<Output>;
}

fn main() {}
