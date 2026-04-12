use rostfrei_domain::domain_lifecycle_test;

struct NotALifecycle;

#[domain_lifecycle_test(NotALifecycle)]
fn non_lifecycle() {}

fn main() {}
