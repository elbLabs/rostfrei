use super::DomainServiceDescriptor;
use crate::{ActionDescriptor, BoundedContextType, DecisionDescriptor};

pub trait DomainServiceType: 'static {
    type Context: BoundedContextType;

    const DESCRIPTOR: DomainServiceDescriptor;
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[];
    const DECISION_CONTRACTS: &'static [&'static [DecisionDescriptor]] = &[];
}
