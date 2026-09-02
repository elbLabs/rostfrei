use crate::{AggregateId, DomainServiceId, EntityId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ActionOwnerId {
    Aggregate(AggregateId),
    DomainService(DomainServiceId),
    Entity(EntityId),
}
