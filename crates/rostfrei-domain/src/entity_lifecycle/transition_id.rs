use super::EntityLifecycleId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntityLifecycleTransitionId {
    pub lifecycle: EntityLifecycleId,
    pub local: &'static str,
}
