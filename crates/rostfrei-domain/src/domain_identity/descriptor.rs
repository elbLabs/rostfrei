use crate::ScalarType;

use super::DomainIdentityId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainIdentityDescriptor {
    pub id: DomainIdentityId,
    pub scalar: ScalarType,
}
