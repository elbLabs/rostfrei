use domain::StateTransition;

#[derive(StateTransition)]
#[transition(state = Workflow)]
enum WorkflowTransition {
    #[transition(id = "start", label = "Start")]
    #[transition(id = "begin", label = "Begin")]
    #[edge(from = Draft, to = Active)]
    Start,
}

struct Workflow;

fn main() {}

rostfrei_domain_macros::__install_test_macro_support!();
