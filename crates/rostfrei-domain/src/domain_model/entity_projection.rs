use serde_json::{Value, json};

use crate::{EntityDescriptor, EntityId, EntityLifecycleDescriptor};

use super::{
    error::DomainModelError,
    field_projection,
    id_projection::{domain_identity as domain_identity_id, entity as entity_id},
    lifecycle_action_validation::LifecycleActionInventory,
    lifecycle_projection::LifecycleProjection,
};

pub(super) struct EntityProjection {
    entities: Vec<(EntityId, Value)>,
    lifecycles: LifecycleProjection,
}

impl EntityProjection {
    pub(super) const fn new() -> Self {
        Self {
            entities: Vec::new(),
            lifecycles: LifecycleProjection::new(),
        }
    }

    pub(super) fn add(&mut self, descriptor: EntityDescriptor) {
        self.entities.push((descriptor.id, entity(descriptor)));
    }

    pub(super) fn add_with_lifecycle(
        &mut self,
        descriptor: EntityDescriptor,
        lifecycle: Option<EntityLifecycleDescriptor>,
    ) -> Result<(), DomainModelError> {
        let mut value = entity(descriptor);
        if let (Some(lifecycle), Some(object)) = (lifecycle, value.as_object_mut()) {
            object.insert(
                "lifecycle".to_owned(),
                self.lifecycles.add(descriptor.id, lifecycle)?,
            );
        }
        self.entities.push((descriptor.id, value));
        Ok(())
    }

    pub(super) fn ids(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entities.iter().map(|(id, _)| *id)
    }

    pub(super) fn validate_lifecycle_actions(
        &self,
        inventory: &LifecycleActionInventory,
    ) -> Result<(), DomainModelError> {
        self.lifecycles.validate_actions(inventory)
    }

    pub(super) fn into_values(self) -> Vec<Value> {
        self.entities.into_iter().map(|(_, value)| value).collect()
    }
}

fn entity(descriptor: EntityDescriptor) -> Value {
    json!({
        "id": entity_id(descriptor.id),
        "label": descriptor.label,
        "identity": {
            "field": descriptor.identity.field,
            "id": domain_identity_id(descriptor.identity.identity),
        },
        "fields": field_projection::fields(descriptor.fields),
    })
}
