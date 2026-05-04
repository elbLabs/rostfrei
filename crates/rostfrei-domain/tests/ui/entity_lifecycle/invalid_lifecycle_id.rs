use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "Invalid", label = "Workflow")]
#[lifecycle(initial = Draft)]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
