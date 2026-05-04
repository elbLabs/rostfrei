use domain::StateTransition;

#[derive(StateTransition)]
#[transition(state = Workflow)]
enum WorkflowTransition {
    #[edge(from = Draft, to = Active)]
    Start,
}

struct Workflow;

fn main() {}

rostfrei_domain_macros::__install_test_macro_support!();
