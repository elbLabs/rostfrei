use domain::domain_policy;

#[domain_policy(id = "evaluate", label = "Evaluate")]
struct NotATrait;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
