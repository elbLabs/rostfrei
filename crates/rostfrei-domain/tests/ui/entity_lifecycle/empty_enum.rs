use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
enum Workflow {}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
