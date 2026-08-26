use super::{EntityId, IdentityDescriptor};
use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityDescriptor {
    pub id: EntityId,
    pub label: &'static str,
    pub identity: IdentityDescriptor,
    pub fields: &'static [FieldDescriptor],
}
