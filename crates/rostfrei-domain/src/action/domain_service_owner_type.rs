use super::PublicActionOwnerType;
use crate::DomainServiceType;

pub trait DomainServiceActionOwnerType: PublicActionOwnerType + DomainServiceType {}
