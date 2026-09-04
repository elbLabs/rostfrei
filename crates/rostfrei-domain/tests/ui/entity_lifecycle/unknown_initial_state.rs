use domain::EntityLifecycle;

#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "workflow", label = "Workflow")]
#[lifecycle(initial = Missing)]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
