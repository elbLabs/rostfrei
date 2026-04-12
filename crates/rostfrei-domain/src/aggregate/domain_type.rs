use super::AggregateDescriptor;
use crate::{
    ActionDescriptor, BoundedContextType, DecisionDescriptor, EntityType, InvariantDescriptor,
};

pub trait AggregateType: 'static + Sized {
    type Context: BoundedContextType;
    type Root: EntityType<Owner = Self>;

    const DESCRIPTOR: AggregateDescriptor;
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[];
    const DECISION_CONTRACTS: &'static [&'static [DecisionDescriptor]] = &[];
    const INVARIANT_CONTRACTS: &'static [&'static [InvariantDescriptor]] = &[];
}
