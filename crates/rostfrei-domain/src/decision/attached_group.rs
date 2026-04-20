use super::{DecisionGroupType, DecisionOwnerType};

#[doc(hidden)]
pub trait AttachedDecisionGroup<Group>: DecisionOwnerType
where
    Group: DecisionGroupType<Owner = Self>,
{
}
