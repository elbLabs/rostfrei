use domain::EntityLifecycle;

#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "workflow", label = "Workflow")]
#[lifecycle(initial = Draft)]
#[lifecycle(initial = Active)]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
    #[state(id = "active", label = "Active")]
    Active,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
