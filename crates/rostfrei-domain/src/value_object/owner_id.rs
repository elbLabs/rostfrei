use crate::{AggregateId, BoundedContextId, EntityId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ValueObjectOwnerId {
    BoundedContext(BoundedContextId),
    Aggregate(AggregateId),
    Entity(EntityId),
}
