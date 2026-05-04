use domain::StateTransition;

#[derive(StateTransition)]
enum WorkflowTransition {
    #[transition(id = "start", label = "Start")]
    #[edge(from = Draft, to = Active)]
    Start,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
