use rostfrei_domain::{BoundedContext, DomainService, ValueObject, domain_decisions};

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
    #[decision(id = "first", label = "First")]
    fn first(input: Input) -> Output;

    #[decision(id = "second", label = "Second")]
    fn second(input: Input) -> Output;
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
    fn first(input: Input) -> Output {
        Output(input.0)
    }
}

fn main() {}
