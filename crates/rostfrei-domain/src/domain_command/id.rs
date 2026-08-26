use super::DomainCommandOwnerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DomainCommandId {
    pub owner: DomainCommandOwnerId,
    pub local: &'static str,
}
