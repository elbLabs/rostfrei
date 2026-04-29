use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "consume", label = "Consume")]
    fn consume(self) -> Self;
}

#[derive(DomainIdentity)]
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
    fn consume(self) -> Self {
        self
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
