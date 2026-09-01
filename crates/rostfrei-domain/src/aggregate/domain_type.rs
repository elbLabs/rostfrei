use super::AggregateDescriptor;
use crate::{ActionDescriptor, DecisionDescriptor, InvariantDescriptor};

pub trait AggregateType: 'static + Sized {
    const DESCRIPTOR: AggregateDescriptor;
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[];
    const DECISION_GROUPS: &'static [&'static [DecisionDescriptor]] = &[];
    const INVARIANT_CONTRACTS: &'static [&'static [InvariantDescriptor]] = &[];
}
