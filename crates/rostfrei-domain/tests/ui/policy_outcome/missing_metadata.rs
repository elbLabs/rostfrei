use domain::PolicyOutcome;

#[derive(PolicyOutcome)]
enum Outcome {
    Missing,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
