use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
#[lifecycle(initial = Draft)]
enum Workflow {
    #[state(id = "draft", label = " ")]
    Draft,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
