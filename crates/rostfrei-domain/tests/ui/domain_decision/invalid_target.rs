use domain::domain_decision;

#[domain_decision(id = "decide", label = "Decide")]
struct NotATrait;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
