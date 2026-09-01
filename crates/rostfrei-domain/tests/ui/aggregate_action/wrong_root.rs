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

pub struct WrongRoot;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut WrongRoot);
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

impl Actions for Owner {
    fn change(root: &mut WrongRoot) {
        let _ = root;
    }
}

fn main() {}
