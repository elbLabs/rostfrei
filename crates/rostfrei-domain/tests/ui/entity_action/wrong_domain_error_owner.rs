use rostfrei_domain::{
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
#[domain(owner = Item)]
struct ItemId(u8);

#[derive(Entity)]
#[domain(id = "item", label = "Item", owner = Owner, actions = [Actions])]
struct Item {
    #[domain(identity)]
    id: ItemId,
}

impl Actions for Item {
    fn rename(&self) -> Result<(), WrongError> {
        Ok(())
    }
}

#[derive(DomainIdentity)]
#[domain(owner = Other)]
struct OtherId(u8);

#[derive(Entity)]
#[domain(id = "other", label = "Other", owner = Owner)]
struct Other {
    #[domain(identity)]
    id: OtherId,
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
#[domain(id = "owner", label = "Owner", context = Context, root = Item)]
struct Owner;

fn main() {}
