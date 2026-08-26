use rostfrei_domain::domain_lifecycle_test;

struct Lifecycle;

#[domain_lifecycle_test(Lifecycle)]
fn invalid_signature<T>() {}

fn main() {}
