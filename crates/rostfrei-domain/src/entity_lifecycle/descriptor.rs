use super::{EntityLifecycleId, EntityLifecycleStateDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityLifecycleDescriptor {
    pub id: EntityLifecycleId,
    pub label: &'static str,
    pub states: &'static [EntityLifecycleStateDescriptor],
}
