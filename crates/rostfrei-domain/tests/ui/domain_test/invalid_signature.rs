use domain::domain_lifecycle_test;

struct Lifecycle;

#[domain_lifecycle_test(Lifecycle)]
fn invalid_signature<T>() {}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
