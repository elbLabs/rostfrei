use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Item)]
struct Id(u8);

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut Item);
}

#[derive(Entity)]
#[domain(id = "item", label = "Item", owner = Owner, actions = [Actions])]
pub struct Item {
    #[domain(identity)]
    id: Id,
}

impl Actions for Item {
    fn change(root: &mut Item) {
        let _ = root;
    }
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Item;
    type Event = domain::NoDomainEvents;
}

fn main() {}
