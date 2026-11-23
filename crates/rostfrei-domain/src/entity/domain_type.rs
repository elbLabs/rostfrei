use super::EntityDescriptor;
use crate::DeclaredFieldType;

pub trait EntityType: 'static {
    const LOCAL_ID: &'static str;
    const DESCRIPTOR: EntityDescriptor;
    const DECLARED_FIELD_TYPES: &'static [DeclaredFieldType] = &[];
}
