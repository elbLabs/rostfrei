use domain::domain_invariant;

#[domain_invariant(id = "valid", label = "Valid")]
struct NotATrait;

fn main() {}
