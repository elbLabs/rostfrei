use super::DomainServiceDescriptor;

pub trait DomainServiceType: 'static + Sized {
    const DESCRIPTOR: DomainServiceDescriptor;
}
