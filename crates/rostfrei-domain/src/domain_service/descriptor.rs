use super::DomainServiceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainServiceDescriptor {
    pub id: DomainServiceId,
    pub label: &'static str,
}
