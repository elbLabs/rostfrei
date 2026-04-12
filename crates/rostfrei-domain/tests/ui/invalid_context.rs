use rostfrei_domain::{Aggregate, DomainIdentity, Entity};

struct Inbox;

#[derive(DomainIdentity)]
#[domain(owner = MailboxRoot)]
struct MailboxId(u64);

#[derive(Entity)]
#[domain(id = "mailbox-root", label = "Mailbox", owner = Mailbox)]
struct MailboxRoot {
    #[domain(identity)]
    id: MailboxId,
}

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox", context = Inbox, root = MailboxRoot)]
struct Mailbox;

fn main() {}
