use domain::{BoundedContext, DomainService, domain_action_test, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "tests", label = "Tests")]
struct Tests;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "run", label = "Run")]
    fn run();
}

#[derive(DomainService)]
#[domain(id = "service", label = "Service")]
struct Service;

impl domain::DomainServiceDefinition for Service {
    type Context = Tests;
}

impl Actions for Service {
    fn run() {}
}

#[domain_action_test(<Service as Actions>::UNKNOWN)]
fn unknown_action_reference() {}

fn main() {}
