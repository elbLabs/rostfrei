use domain::StateTransition;

#[derive(StateTransition)]
#[transition(state = Workflow)]
enum WorkflowTransition {
    #[transition(id = "change", label = "Start")]
    #[edge(from = Draft, to = Active)]
    Start,
    #[transition(id = "change", label = "Finish")]
    #[edge(from = Active, to = Done)]
    Finish,
}

struct Workflow;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
