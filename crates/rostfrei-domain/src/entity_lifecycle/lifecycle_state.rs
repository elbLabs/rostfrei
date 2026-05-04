use super::{
    EntityLifecycleStateId, EntityLifecycleType, InvalidStateTransition, StateChange,
    StateTransition,
};

pub trait LifecycleState: EntityLifecycleType + Copy + Eq {
    const INITIAL: Self;

    fn state_id(self) -> EntityLifecycleStateId;

    fn evaluate<Transition>(
        self,
        transition: &Transition,
    ) -> Result<StateChange<Self>, InvalidStateTransition>
    where
        Transition: StateTransition<State = Self>,
    {
        transition.evaluate(self)
    }
}
