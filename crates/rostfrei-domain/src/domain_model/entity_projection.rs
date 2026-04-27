use serde_json::{Value, json};

use crate::{EntityDescriptor, EntityId};

use super::{
    field_projection,
    id_projection::{domain_identity as domain_identity_id, entity as entity_id},
};

pub(super) struct EntityProjection {
    entities: Vec<(EntityId, Value)>,
}

impl EntityProjection {
    pub(super) const fn new() -> Self {
        Self {
            entities: Vec::new(),
        }
    }

    pub(super) fn add(&mut self, descriptor: EntityDescriptor) {
        self.entities.push((descriptor.id, entity(descriptor)));
    }

    pub(super) fn ids(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entities.iter().map(|(id, _)| *id)
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
