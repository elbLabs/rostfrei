use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
struct Root {
    #[domain(identity)]
    id: Id,
}

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute();
}

#[derive(Aggregate)]
#[domain(
    id = "owner",
    label = "Owner",
    context = Context,
    root = Root,
    actions = [Actions]
)]
struct Owner;

impl Actions for Owner {
    fn execute() {}
}

fn main() {}
