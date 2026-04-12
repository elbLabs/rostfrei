use domain::{BoundedContext, DomainService};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

struct Input;
struct Output;

trait Decisions {
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
        Output
    }
}

fn main() {}
