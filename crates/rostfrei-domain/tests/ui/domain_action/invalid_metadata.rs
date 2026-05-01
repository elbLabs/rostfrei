use domain::domain_action;

#[domain_action(id = "missing-label")]
trait MissingLabel {
    fn execute();
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
