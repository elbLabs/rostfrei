use domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox(String);

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
struct Mailbox { name: String }

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
