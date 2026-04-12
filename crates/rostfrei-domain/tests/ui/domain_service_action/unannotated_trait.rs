use domain::{BoundedContext, DomainService};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

trait Actions {
    fn execute();
}

#[derive(DomainService)]
#[domain(id = "service", label = "Service", context = Context, actions = [Actions])]
struct Service;

impl Actions for Service {
    fn execute() {}
}

fn main() {}
