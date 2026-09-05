use domain::PolicyOutcome;

#[derive(PolicyOutcome)]
struct NotAnEnum;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
