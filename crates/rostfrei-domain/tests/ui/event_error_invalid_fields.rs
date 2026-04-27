use domain::{Aggregate, BoundedContext, DomainError, DomainEvent, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: Id,
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

struct Child;

#[derive(DomainEvent)]
#[domain(id = "invalid-event", label = "Invalid event")]
struct InvalidEvent {
    #[domain(entity)]
    child: Child,
}

#[derive(DomainError)]
#[domain(id = "invalid-error", label = "Invalid error", owner = Owner, code = "INVALID", message = "Invalid.")]
struct InvalidError {
    #[domain(entity)]
    child: Child,
}

fn main() {}
