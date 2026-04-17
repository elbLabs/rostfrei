use serde_json::{Value, json};

use crate::{EntityId, EntityLifecycleDescriptor};

use super::{
    id_projection::action as action_id,
    lifecycle_action_validation::{self, LifecycleActionInventory},
    lifecycle_descriptor_validation,
};

pub(super) struct LifecycleProjection {
    descriptors: Vec<EntityLifecycleDescriptor>,
}

impl LifecycleProjection {
    pub(super) const fn new() -> Self {
        Self {
            descriptors: Vec::new(),
        }
    }

    pub(super) fn add(
        &mut self,
        expected_owner: EntityId,
        descriptor: EntityLifecycleDescriptor,
    ) -> Value {
        lifecycle_descriptor_validation::validate(expected_owner, descriptor);
        self.descriptors.push(descriptor);
        lifecycle(descriptor)
    }

    pub(super) fn validate_actions(&self, inventory: &LifecycleActionInventory) {
        lifecycle_action_validation::validate(self.descriptors.iter().copied(), inventory);
    }
}

fn lifecycle(descriptor: EntityLifecycleDescriptor) -> Value {
    json!({
        "id": descriptor.id.local,
        "label": descriptor.label,
        "states": descriptor.states.iter().map(|state| json!({
            "id": state.id.local,
            "label": state.label,
        })).collect::<Vec<_>>(),
        "initial": descriptor.initial.local,
        "transitions": descriptor.transitions.iter().map(|transition| json!({
            "source": transition.source.local,
            "action": action_id(transition.action),
            "target": transition.target.local,
        })).collect::<Vec<_>>(),
    })
}
