use crate::{AggregateId, DomainServiceId, EntityId, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ActionOwnerId {
    Aggregate(AggregateId),
    DomainService(DomainServiceId),
    Entity(EntityId),
    ValueObject(ValueObjectId),
}
