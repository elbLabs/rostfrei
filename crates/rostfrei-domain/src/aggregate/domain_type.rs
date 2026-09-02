use super::AggregateDescriptor;
use crate::DecisionDescriptor;

pub trait AggregateType: 'static + Sized {
    const DESCRIPTOR: AggregateDescriptor;
    const DECISION_GROUPS: &'static [&'static [DecisionDescriptor]] = &[];
}
