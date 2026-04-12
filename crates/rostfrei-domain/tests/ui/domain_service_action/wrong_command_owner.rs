use rostfrei_domain::{BoundedContext, DomainCommand, DomainService, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainService)]
#[domain(id = "other", label = "Other", context = Context)]
struct Other;

#[derive(DomainCommand)]
#[domain(id = "execute", label = "Execute", owner = Other)]
pub struct Execute;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute(input: Execute);
}

#[derive(DomainService)]
#[domain(id = "service", label = "Service", context = Context, actions = [Actions])]
struct Service;

impl Actions for Service {
    fn execute(input: Execute) {
        let _ = input;
    }
}

fn main() {}
