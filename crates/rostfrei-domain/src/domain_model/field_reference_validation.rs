use crate::{AggregateId, EntityId};

use super::{
    error::{DomainModelError, DomainModelReference},
    field_reference_collection::{FieldDescriptorLocation, FieldReference, FieldReferenceRecord},
};

pub(super) struct FieldReferenceInventory {
    entities: Vec<EntityId>,
    aggregates: Vec<AggregateId>,
}

impl FieldReferenceInventory {
    pub(super) const fn new(entities: Vec<EntityId>, aggregates: Vec<AggregateId>) -> Self {
        Self {
            entities,
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
            FieldReference::Entity(id) => {
                if !inventory.entities.contains(&id) {
                    return Err(missing_reference(
                        DomainModelReference::Entity(Box::new(id)),
                        record.location,
                        "entities",
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
