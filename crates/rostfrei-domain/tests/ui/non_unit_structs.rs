use rostfrei_domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox(String);

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox", context = Inbox, root = MailboxRoot)]
struct Mailbox { name: String }

fn main() {}
