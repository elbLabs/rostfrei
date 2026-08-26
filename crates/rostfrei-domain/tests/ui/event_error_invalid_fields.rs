use domain::{Aggregate, BoundedContext, DomainError, DomainEvent, DomainIdentity, Entity};

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

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

struct Child;

#[derive(DomainEvent)]
#[domain(id = "invalid-event", label = "Invalid event", owner = Owner)]
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
