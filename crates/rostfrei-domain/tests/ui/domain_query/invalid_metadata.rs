use domain::domain_query;

#[domain_query(label = "Missing ID")]
trait MissingId {
    fn execute();
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
