use rostfrei_domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "consume", label = "Consume")]
    fn consume(self) -> Self;
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
    fn consume(self) -> Self {
        self
    }
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Item)]
struct Owner;

fn main() {}
