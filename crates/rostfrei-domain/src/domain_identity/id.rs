use crate::EntityId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DomainIdentityId {
    pub owner: EntityId,
}
