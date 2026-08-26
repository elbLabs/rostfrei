use crate::EntityId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntityLifecycleId {
    pub owner: EntityId,
    pub local: &'static str,
}
