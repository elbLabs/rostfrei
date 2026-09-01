use super::InvariantOwnerType;
use crate::{AggregateDefinition, AggregateType};

pub trait AggregateInvariantOwnerType:
    AggregateType
    + AggregateDefinition
    + InvariantOwnerType<Candidate = <Self as AggregateDefinition>::Root>
{
}
