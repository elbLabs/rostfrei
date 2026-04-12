use domain::domain_lifecycle_test;

struct Lifecycle;

#[domain_lifecycle_test(Lifecycle)]
struct NotAFunction;

fn main() {}
