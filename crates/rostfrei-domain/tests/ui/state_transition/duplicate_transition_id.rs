use domain::StateTransition;

#[derive(StateTransition)]
#[transition(state = Workflow)]
enum WorkflowTransition {
    #[edge(id = "change", label = "Start", from = Draft, to = Active)]
    Start,
    #[edge(id = "change", label = "Finish", from = Active, to = Done)]
    Finish,
}

struct Workflow;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
