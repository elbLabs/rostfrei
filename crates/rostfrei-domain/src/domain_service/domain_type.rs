use super::DomainServiceDescriptor;
use crate::{ActionDescriptor, BoundedContextType};

pub trait DomainServiceType: 'static {
    type Context: BoundedContextType;

    const DESCRIPTOR: DomainServiceDescriptor;
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[];
}
