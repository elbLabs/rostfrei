use rostfrei_domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
struct Inbox;

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
struct Mailbox;

fn main() {}
