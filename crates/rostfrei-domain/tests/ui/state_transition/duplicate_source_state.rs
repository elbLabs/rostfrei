use domain::StateTransition;

#[derive(StateTransition)]
#[transition(state = Workflow)]
enum WorkflowTransition {
    #[transition(id = "start", label = "Start")]
    #[edge(from = Draft, to = Active)]
    #[edge(from = Draft, to = Done)]
    Start,
}

struct Workflow;

fn main() {}

rostfrei_domain_macros::__install_test_macro_support!();
