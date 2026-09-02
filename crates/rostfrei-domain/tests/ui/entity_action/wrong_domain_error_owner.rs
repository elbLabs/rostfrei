use domain::{
    Aggregate, BoundedContext, DomainError, DomainIdentity, Entity, domain_actions,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "rename", label = "Rename")]
    fn rename(&self) -> Result<(), WrongError>;
}

#[derive(DomainIdentity)]
struct ItemId(u8);

#[derive(Entity)]
#[domain(id = "item", label = "Item")]
struct Item {
    #[domain(identity)]
    id: ItemId,
}

impl domain::EntityDefinition for Item {
    type Owner = Owner;
    type Identity = ItemId;
}

impl Actions for Item {
    fn rename(&self) -> Result<(), WrongError> {
        Ok(())
    }
}

#[derive(DomainIdentity)]
struct OtherId(u8);

#[derive(Entity)]
#[domain(id = "other", label = "Other")]
struct Other {
    #[domain(identity)]
    id: OtherId,
}

impl domain::EntityDefinition for Other {
    type Owner = Owner;
    type Identity = OtherId;
}

#[derive(DomainError)]
#[domain(
    id = "wrong",
    label = "Wrong",
    owner = Other,
    code = "WRONG",
    message = "Wrong."
)]
struct WrongError;

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Item;
    type Event = domain::NoDomainEvents;
}

fn main() {}
