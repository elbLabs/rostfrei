use super::EntityLifecycleId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntityLifecycleStateId {
    pub lifecycle: EntityLifecycleId,
    pub local: &'static str,
}
