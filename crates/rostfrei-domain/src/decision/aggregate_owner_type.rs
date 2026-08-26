use super::DecisionOwnerType;
use crate::AggregateType;

pub trait AggregateDecisionOwnerType: DecisionOwnerType + AggregateType {}
