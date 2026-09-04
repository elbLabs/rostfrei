use super::{EntityLifecycleId, EntityLifecycleStateDescriptor, EntityLifecycleStateId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityLifecycleDescriptor {
    pub id: EntityLifecycleId,
    pub label: &'static str,
    pub initial: EntityLifecycleStateId,
    pub states: &'static [EntityLifecycleStateDescriptor],
}
