use domain::domain_invariant;

#[domain_invariant(id = "valid", label = "Valid")]
struct NotATrait;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
