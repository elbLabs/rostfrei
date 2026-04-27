use super::EntityType;
use crate::{AggregateType, DomainIdentityType};

/// Supplies the owner and identity relationships of a modeled entity.
pub trait EntityDefinition: EntityType {
    type Owner: AggregateType;
    type Identity: DomainIdentityType<Owner = Self>;
}
