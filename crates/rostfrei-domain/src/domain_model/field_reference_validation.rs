use crate::{AggregateId, DomainIdentityId, EntityId, ValueObjectId};

use super::{
    error::{DomainModelError, DomainModelReference},
    field_reference_collection::{FieldDescriptorLocation, FieldReference, FieldReferenceRecord},
};

pub(super) struct FieldReferenceInventory {
    identities: Vec<DomainIdentityId>,
    entities: Vec<EntityId>,
    value_objects: Vec<ValueObjectId>,
    aggregates: Vec<AggregateId>,
}

impl FieldReferenceInventory {
    pub(super) const fn new(
        identities: Vec<DomainIdentityId>,
        entities: Vec<EntityId>,
        value_objects: Vec<ValueObjectId>,
        aggregates: Vec<AggregateId>,
    ) -> Self {
        Self {
            identities,
            entities,
            value_objects,
            aggregates,
        }
    }
}

pub(super) fn validate(
    references: impl IntoIterator<Item = FieldReferenceRecord>,
    inventory: &FieldReferenceInventory,
) -> Result<(), DomainModelError> {
    for record in references {
        match record.reference {
            FieldReference::DomainIdentity(id) => {
                if !inventory.identities.contains(&id) {
                    return Err(missing_reference(
                        DomainModelReference::DomainIdentity(Box::new(id)),
                        record.location,
                        "identities",
                    ));
                }
            }
            FieldReference::Entity(id) => {
                if !inventory.entities.contains(&id) {
                    return Err(missing_reference(
                        DomainModelReference::Entity(Box::new(id)),
                        record.location,
                        "entities",
                    ));
                }
            }
            FieldReference::ValueObject(id) => {
                if !inventory.value_objects.contains(&id) {
                    return Err(missing_reference(
                        DomainModelReference::ValueObject(Box::new(id)),
                        record.location,
                        "value_objects",
                    ));
                }
            }
            FieldReference::Aggregate(id) => {
                if !inventory.aggregates.contains(&id) {
                    return Err(missing_reference(
                        DomainModelReference::Aggregate(Box::new(id)),
                        record.location,
                        "aggregates",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn missing_reference(
    reference: DomainModelReference,
    location: FieldDescriptorLocation,
    inventory_key: &'static str,
) -> DomainModelError {
    DomainModelError::FieldReferenceInventoryViolation {
        reference,
        location: location.to_string(),
        inventory_key,
    }
}
