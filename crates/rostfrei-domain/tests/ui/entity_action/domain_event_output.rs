use rostfrei_domain::{
    Aggregate, BoundedContext, DomainEvent, DomainIdentity, Entity, domain_actions,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "publish", label = "Publish")]
    fn publish(&self) -> Published;
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
    fn publish(&self) -> Published {
        Published
    }
}

#[derive(DomainEvent)]
#[domain(id = "published", label = "Published", owner = Owner)]
struct Published;

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Item)]
struct Owner;

fn main() {}
