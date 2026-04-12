use super::EntityLifecycleStateId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityLifecycleStateDescriptor {
    pub id: EntityLifecycleStateId,
    pub label: &'static str,
}
