use domain::{Aggregate, BoundedContext};

#[derive(BoundedContext)]
#[domain(id = "Customer--Support", label = "Customer Support")]
struct CustomerSupport;

#[derive(Aggregate)]
#[domain(id = "", label = "Mailbox")]
struct Mailbox;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
