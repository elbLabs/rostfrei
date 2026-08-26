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

fn main() {}
