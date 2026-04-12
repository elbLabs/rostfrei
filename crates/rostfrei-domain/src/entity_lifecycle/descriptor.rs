use super::{
    EntityLifecycleId, EntityLifecycleStateDescriptor, EntityLifecycleStateId,
    EntityLifecycleTransitionDescriptor,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityLifecycleDescriptor {
    pub id: EntityLifecycleId,
    pub label: &'static str,
    pub states: &'static [EntityLifecycleStateDescriptor],
    pub initial: EntityLifecycleStateId,
    pub transitions: &'static [EntityLifecycleTransitionDescriptor],
}
