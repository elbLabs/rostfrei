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
    #[state(id = "completed", label = "Completed")]
    Completed,
}

#[derive(domain::StateTransition)]
#[transition(state = Workflow)]
enum WorkflowTransition {
    #[transition(id = "start", label = "Start")]
    #[edge(from = Draft, to = Active)]
    #[edge(from = Completed, to = Active)]
    Start,
}

fn main() {
    assert_eq!(
        Workflow::Draft.evaluate(&WorkflowTransition::Start),
        Ok(StateChange::new(Workflow::Draft, Workflow::Active))
    );
    assert_eq!(
        Workflow::Completed.evaluate(&WorkflowTransition::Start),
        Ok(StateChange::new(Workflow::Completed, Workflow::Active))
    );
    assert_eq!(WorkflowTransition::DESCRIPTORS.len(), 1);
    assert_eq!(WorkflowTransition::Start.descriptor().edges.len(), 2);
}

rostfrei_domain_macros::__install_test_macro_support!();
