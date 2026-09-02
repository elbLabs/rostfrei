/// Marks a Rust type as a domain identity.
pub trait DomainIdentity: 'static + Sized {}

/// Binds an identity marker to the Entity that declares it.
#[doc(hidden)]
pub trait DomainIdentityType: DomainIdentity {
    type Owner: crate::EntityDefinition<Identity = Self>;

    const DESCRIPTOR: super::DomainIdentityDescriptor;
}
