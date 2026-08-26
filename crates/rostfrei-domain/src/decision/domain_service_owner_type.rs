use super::DecisionOwnerType;
use crate::DomainServiceType;

pub trait DomainServiceDecisionOwnerType: DecisionOwnerType + DomainServiceType {}
