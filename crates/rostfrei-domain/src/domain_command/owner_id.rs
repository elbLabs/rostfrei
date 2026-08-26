use crate::{AggregateId, DomainServiceId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DomainCommandOwnerId {
    Aggregate(AggregateId),
    DomainService(DomainServiceId),
}
