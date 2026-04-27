use domain::{
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
#[domain(id = "item", label = "Item")]
struct Item {
    #[domain(identity)]
    id: Id,
}

impl domain::EntityDefinition for Item {
    type Owner = Owner;
    type Identity = Id;
}

impl Actions for Item {
    fn publish(&self) -> Published {
        Published
    }
}

#[derive(DomainEvent)]
#[domain(id = "published", label = "Published")]
struct Published;

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Item;
    type Event = OwnerEvents;
}

#[derive(domain::AggregateEvents)]
enum OwnerEvents {
    Event0(Published),
}

fn main() {}
