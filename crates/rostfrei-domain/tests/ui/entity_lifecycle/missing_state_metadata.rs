use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
enum Workflow {
    Draft,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
