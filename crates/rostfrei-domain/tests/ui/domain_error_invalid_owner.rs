use rostfrei_domain::{BoundedContext, DomainError};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainError)]
#[domain(id = "denied", label = "Denied", owner = Inbox, code = "DENIED", message = "Denied.")]
struct Denied;

fn main() {}
