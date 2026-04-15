use crate::{AggregateId, EntityId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DecisionOwnerId {
    Aggregate(AggregateId),
    Entity(EntityId),
}
