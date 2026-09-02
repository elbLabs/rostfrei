use super::DomainServiceType;
use crate::BoundedContextType;

/// Supplies the bounded-context relationship of a modeled domain service.
pub trait DomainServiceDefinition: DomainServiceType {
    type Context: BoundedContextType;
}
