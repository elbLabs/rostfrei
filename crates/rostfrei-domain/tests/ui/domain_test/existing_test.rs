use rostfrei_domain::domain_lifecycle_test;

struct Lifecycle;

#[domain_lifecycle_test(Lifecycle)]
#[test]
fn existing_test() {}

fn main() {}
