use domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
struct Inbox;

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
struct Mailbox;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
