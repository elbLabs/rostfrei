use rostfrei_domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

trait Actions {
    fn rename(&self);
}

#[derive(DomainIdentity)]
#[domain(owner = Item)]
struct Id(u8);

#[derive(Entity)]
#[domain(id = "item", label = "Item", owner = Owner, actions = [Actions])]
struct Item {
    #[domain(identity)]
    id: Id,
}

impl Actions for Item {
    fn rename(&self) {}
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Item)]
struct Owner;

fn main() {}
