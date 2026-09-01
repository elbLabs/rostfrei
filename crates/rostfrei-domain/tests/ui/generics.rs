use domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox<T>;

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
struct Mailbox<T>;

fn main() {}
