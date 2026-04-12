use domain::{BoundedContext, DomainError, DomainService, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainService)]
#[domain(id = "other", label = "Other", context = Context)]
struct Other;

#[derive(DomainError)]
#[domain(
    id = "denied",
    label = "Denied",
    owner = Other,
    code = "DENIED",
    message = "Denied."
)]
pub struct Denied;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute() -> Result<(), Denied>;
}

#[derive(DomainService)]
#[domain(id = "service", label = "Service", context = Context, actions = [Actions])]
struct Service;

impl Actions for Service {
    fn execute() -> Result<(), Denied> {
        Ok(())
    }
}

fn main() {}
