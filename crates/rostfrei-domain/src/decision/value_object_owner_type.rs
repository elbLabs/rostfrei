use super::DecisionOwnerType;
use crate::ValueObjectType;

pub trait ValueObjectDecisionOwnerType: DecisionOwnerType + ValueObjectType {}
