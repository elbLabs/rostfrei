use domain::{BoundedContext, DomainEvent};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainEvent)]
#[domain(id = "created", label = "Created", owner = Inbox)]
struct Created;

fn main() {}
