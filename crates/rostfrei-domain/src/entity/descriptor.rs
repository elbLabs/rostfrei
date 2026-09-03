use super::EntityId;
use crate::{DomainIdentityId, FieldDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityDescriptor {
    pub id: EntityId,
    pub label: &'static str,
    pub identity: DomainIdentityId,
    pub fields: &'static [FieldDescriptor],
}
