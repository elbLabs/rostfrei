use domain::domain_action;

#[domain_action(id = "execute", label = "Execute")]
struct NotATrait;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
