use crate::{AggregateId, EntityId, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InvariantOwnerId {
    Aggregate(AggregateId),
    Entity(EntityId),
    ValueObject(ValueObjectId),
}
