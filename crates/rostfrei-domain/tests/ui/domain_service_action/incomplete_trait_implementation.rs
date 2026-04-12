use rostfrei_domain::{BoundedContext, DomainService, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute();

    #[action(id = "cancel", label = "Cancel")]
    fn cancel();
}

#[derive(DomainService)]
#[domain(id = "service", label = "Service", context = Context, actions = [Actions])]
struct Service;

impl Actions for Service {
    fn execute() {}
}

fn main() {}
