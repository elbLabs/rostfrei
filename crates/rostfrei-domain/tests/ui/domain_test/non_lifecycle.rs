use domain::domain_lifecycle_test;

struct NotALifecycle;

#[domain_lifecycle_test(NotALifecycle)]
fn non_lifecycle() {}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
