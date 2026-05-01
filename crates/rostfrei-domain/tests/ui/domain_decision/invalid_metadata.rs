use domain::domain_decision;

#[domain_decision(id = "missing-label")]
trait MissingLabel {
    fn decide();
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
