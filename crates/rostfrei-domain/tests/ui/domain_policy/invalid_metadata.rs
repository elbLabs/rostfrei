use domain::domain_policy;

#[domain_policy(id = "missing-label")]
trait MissingLabel {
    fn evaluate();
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
