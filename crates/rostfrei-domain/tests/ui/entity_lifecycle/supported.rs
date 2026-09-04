use domain::{EntityLifecycle, EntityLifecycleType, LifecycleState};

#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "workflow", label = "Workflow")]
#[lifecycle(initial = Draft)]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
    #[state(id = "active", label = "Active")]
    Active,
}

const _: domain::EntityLifecycleDescriptor = Workflow::DESCRIPTOR;
const _: Workflow = Workflow::INITIAL;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
