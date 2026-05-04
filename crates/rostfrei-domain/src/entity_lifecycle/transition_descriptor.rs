use super::{EntityLifecycleTransitionId, StateTransitionEdge};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateTransitionDescriptor<State: 'static> {
    pub id: EntityLifecycleTransitionId,
    pub label: &'static str,
    pub edges: &'static [StateTransitionEdge<State>],
}
