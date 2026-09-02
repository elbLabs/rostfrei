use super::DomainIdentityId;

/// Entity-scoped metadata generated when an identity is bound to an Entity.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainIdentityDescriptor {
    pub id: DomainIdentityId,
}
