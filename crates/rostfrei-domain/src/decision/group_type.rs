use super::{DecisionDescriptor, DecisionOwnerType};

pub trait DecisionGroupType: 'static {
    type Owner: DecisionOwnerType;

    const DECISIONS: &'static [DecisionDescriptor];
}
