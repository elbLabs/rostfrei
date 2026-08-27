use super::{DecisionDescriptor, DecisionOwnerType};

#[doc(hidden)]
pub trait DecisionProvider: DecisionOwnerType {
    const DECISIONS: &'static [DecisionDescriptor];
}

#[doc(hidden)]
pub trait AttachedDecisionProvider: DecisionProvider {}
