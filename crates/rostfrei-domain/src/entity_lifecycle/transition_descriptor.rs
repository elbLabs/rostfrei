use super::EntityLifecycleStateId;
use crate::ActionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityLifecycleTransitionDescriptor {
    pub source: EntityLifecycleStateId,
    pub action: ActionId,
    pub target: EntityLifecycleStateId,
}
