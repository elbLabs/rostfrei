use domain::{
    EntityLifecycle, LifecycleState, StateChange, StateTransition as StateTransitionType,
};

#[derive(EntityLifecycle, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "workflow", label = "Workflow")]
#[lifecycle(initial = Draft)]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
    #[state(id = "active", label = "Active")]
    Active,
}

#[derive(domain::StateTransition)]
#[transition(state = Workflow)]
enum WorkflowTransition {
    #[edge(id = "start", label = "Start", from = Draft, to = Active)]
    Start,
}

fn main() {
    assert_eq!(
        Workflow::Draft.evaluate(&WorkflowTransition::Start),
        Ok(StateChange::new(Workflow::Draft, Workflow::Active))
    );
    assert_eq!(WorkflowTransition::DESCRIPTORS.len(), 1);
}

rostfrei_domain_macros::__install_test_macro_support!();
