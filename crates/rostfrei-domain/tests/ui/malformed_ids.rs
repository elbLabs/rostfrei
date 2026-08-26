use domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
#[domain(id = "Customer--Support", label = "Customer Support")]
struct CustomerSupport;

#[derive(Aggregate)]
#[domain(id = "", label = "Mailbox", context = CustomerSupport, root = MailboxRoot)]
struct Mailbox;

fn main() {}
