use domain::PolicyOutcome;

#[derive(PolicyOutcome)]
enum Outcome {
    #[outcome(id = "same", label = "First")]
    First,
    #[outcome(id = "same", label = "Second")]
    Second,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
