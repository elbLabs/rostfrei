use domain::domain_query;

#[domain_query(id = "available", label = "Available")]
struct NotATrait;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
