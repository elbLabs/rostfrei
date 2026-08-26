use crate::{AggregateId, DomainServiceId, EntityId, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DomainErrorOwnerId {
    DomainService(DomainServiceId),
    Aggregate(AggregateId),
    Entity(EntityId),
    ValueObject(ValueObjectId),
}
