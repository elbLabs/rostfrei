use super::EntityLifecycleTransitionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateTransitionDescriptor<State> {
    pub id: EntityLifecycleTransitionId,
    pub label: &'static str,
    pub from: State,
    pub to: State,
}
