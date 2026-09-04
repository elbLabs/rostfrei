use domain::StateTransition;

#[derive(StateTransition)]
#[transition(state = Workflow)]
struct WorkflowTransition;

struct Workflow;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
