use domain::StateTransition;

#[derive(StateTransition)]
enum WorkflowTransition {
    #[edge(id = "start", label = "Start", from = Draft, to = Active)]
    Start,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
