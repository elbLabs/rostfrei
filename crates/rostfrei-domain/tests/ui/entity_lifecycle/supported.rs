use domain::{EntityLifecycle, EntityLifecycleType};

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
    #[state(id = "active", label = "Active")]
    Active,
}

const _: domain::EntityLifecycleDescriptor = Workflow::DESCRIPTOR;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
