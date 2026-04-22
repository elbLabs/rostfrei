use crate::{AggregateId, DomainServiceId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CommandOwnerId {
    Aggregate(AggregateId),
    DomainService(DomainServiceId),
}
