use crate::DomainIdentityId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityDescriptor {
    pub field: &'static str,
    pub identity: DomainIdentityId,
}
