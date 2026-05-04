use super::{InvalidStateTransition, LifecycleState, StateChange, StateTransitionDescriptor};

pub trait StateTransition: Sized + 'static {
    type State: LifecycleState;

    const DESCRIPTORS: &'static [StateTransitionDescriptor<Self::State>];

    fn descriptor(&self) -> &'static StateTransitionDescriptor<Self::State>;

    fn evaluate(
        &self,
        current: Self::State,
    ) -> Result<StateChange<Self::State>, InvalidStateTransition> {
        let descriptor = self.descriptor();
        descriptor
            .edges
            .iter()
            .find(|edge| current == edge.from)
            .map(|edge| StateChange::new(current, edge.to))
            .ok_or_else(|| InvalidStateTransition::new(current.state_id(), descriptor.id))
    }
}
