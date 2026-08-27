use domain::{BoundedContext, DecisionReference, DomainService, ValueObject, domain_decisions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "input", label = "Input", owner = Context)]
struct Input;

#[derive(ValueObject)]
#[domain(id = "output", label = "Output", owner = Context)]
struct Output;

#[derive(ValueObject)]
#[domain(id = "wrong-output", label = "Wrong output", owner = Context)]
struct WrongOutput;

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
    fn decide(_input: Input) -> Output {
        Output
    }
}

const _: DecisionReference<Service, Input, WrongOutput> =
    <Service as Decisions>::__DOMAIN_DECISION_REFERENCE_DECIDE;

fn main() {}
