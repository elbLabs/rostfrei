use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
#[lifecycle(initial = Draft)]
struct Workflow;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
