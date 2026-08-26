use super::DecisionOwnerType;
use crate::EntityType;

pub trait EntityDecisionOwnerType: DecisionOwnerType + EntityType {}
