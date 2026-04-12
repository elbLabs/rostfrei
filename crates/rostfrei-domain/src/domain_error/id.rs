use super::DomainErrorOwnerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DomainErrorId {
    pub owner: DomainErrorOwnerId,
    pub local: &'static str,
}
