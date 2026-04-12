use super::InvariantOwnerType;
use crate::AggregateType;

pub trait AggregateInvariantOwnerType:
    AggregateType + InvariantOwnerType<Candidate = <Self as AggregateType>::Root>
{
}
