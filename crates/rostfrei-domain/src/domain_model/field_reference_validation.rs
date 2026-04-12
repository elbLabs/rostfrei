use crate::{AggregateId, DomainIdentityId, EntityId, ValueObjectId};

use super::field_reference_collection::{
    FieldDescriptorLocation, FieldReference, FieldReferenceRecord,
};

pub(super) struct FieldReferenceInventory {
    identities: Vec<DomainIdentityId>,
    entities: Vec<EntityId>,
    value_objects: Vec<ValueObjectId>,
    aggregates: Vec<AggregateId>,
}

impl FieldReferenceInventory {
    pub(super) fn new(
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
) {
    for record in references {
        match record.reference {
            FieldReference::DomainIdentity(id) => {
                if !inventory.identities.contains(&id) {
                    missing_reference(id, record.location, "identities");
                }
            }
            FieldReference::Entity(id) => {
                if !inventory.entities.contains(&id) {
                    missing_reference(id, record.location, "entities");
                }
            }
            FieldReference::ValueObject(id) => {
                if !inventory.value_objects.contains(&id) {
                    missing_reference(id, record.location, "value_objects");
                }
            }
            FieldReference::Aggregate(id) => {
                if !inventory.aggregates.contains(&id) {
                    missing_reference(id, record.location, "aggregates");
                }
            }
        }
    }
}

fn missing_reference(
    item_id: impl std::fmt::Debug,
    location: FieldDescriptorLocation,
    inventory_key: &str,
) -> ! {
    panic!(
        "Field reference inventory violation: field references missing {item_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `{inventory_key}`"
    );
}
