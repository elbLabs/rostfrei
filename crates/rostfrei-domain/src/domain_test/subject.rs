use crate::{ActionId, EntityLifecycleId, InvariantId, PolicyId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DomainTestSubject {
    Action(ActionId),
    Policy(PolicyId),
    Invariant(InvariantId),
    Lifecycle(EntityLifecycleId),
}
