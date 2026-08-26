use domain::{BoundedContext, DomainService, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute(&self);
}

#[derive(DomainService)]
#[domain(id = "service", label = "Service", context = Context, actions = [Actions])]
struct Service;

impl Actions for Service {
    fn execute(&self) {}
}

fn main() {}
