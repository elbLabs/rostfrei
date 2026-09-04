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
        if current == descriptor.from {
            Ok(StateChange::new(current, descriptor.to))
        } else {
            Err(InvalidStateTransition::new(
                current.state_id(),
                descriptor.id,
            ))
        }
    }
}
