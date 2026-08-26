use crate::{ActionId, DecisionId, EntityLifecycleId, InvariantId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DomainTestSubject {
    Action(ActionId),
    Decision(DecisionId),
    Invariant(InvariantId),
    Lifecycle(EntityLifecycleId),
}
