use rostfrei_domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox<T>;

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox", context = Inbox<u8>, root = MailboxRoot)]
struct Mailbox<T>;

fn main() {}
