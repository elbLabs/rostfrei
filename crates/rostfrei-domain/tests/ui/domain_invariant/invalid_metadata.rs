use domain::domain_invariant;

#[domain_invariant(label = "Missing ID")]
trait MissingId {
    fn validate();
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
