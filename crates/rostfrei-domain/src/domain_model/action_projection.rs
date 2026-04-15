use serde_json::{Value, json};

use crate::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, ActionOwnerId,
};

use super::{
    action_reference_validation::{self, ActionReferenceInventory},
    field_projection,
    id_projection::{
        action as action_id, domain_error as domain_error_id, domain_event as domain_event_id,
        domain_identity as domain_identity_id, value_object as value_object_id,
    },
};

pub(super) struct ActionProjection {
    registered_owners: Vec<ActionOwnerId>,
    actions: Vec<(ActionDescriptor, Value)>,
    extensions: Vec<(ActionDescriptor, Value)>,
}

impl ActionProjection {
    pub(super) fn new() -> Self {
        Self {
            registered_owners: Vec::new(),
            actions: Vec::new(),
            extensions: Vec::new(),
        }
    }

    pub(super) fn register_owner(&mut self, owner: ActionOwnerId) {
        if !self.registered_owners.contains(&owner) {
            self.registered_owners.push(owner);
        }
    }

    pub(super) fn add_group(
        &mut self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) {
        self.validate_group(expected_owner, descriptors);
        Self::append(&mut self.actions, descriptors);
    }

    pub(super) fn add_extension(
        &mut self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) {
        self.validate_extension(expected_owner, descriptors);
        Self::append(&mut self.extensions, descriptors);
    }

    pub(super) fn attached_ids(&self) -> impl Iterator<Item = ActionId> + '_ {
        self.actions.iter().map(|(descriptor, _)| descriptor.id)
    }

    pub(super) fn extension_ids(&self) -> impl Iterator<Item = ActionId> + '_ {
        self.extensions.iter().map(|(descriptor, _)| descriptor.id)
    }

    pub(super) fn validate_references(&self, inventory: &ActionReferenceInventory) {
        action_reference_validation::validate(
            self.actions
                .iter()
                .chain(&self.extensions)
                .map(|(descriptor, _)| descriptor),
            inventory,
        );
    }

    pub(super) fn into_values(self) -> Vec<Value> {
        self.actions
            .into_iter()
            .chain(self.extensions)
            .map(|(_, value)| value)
            .collect()
    }

    fn validate_extension(
        &self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) {
        if !self.registered_owners.contains(&expected_owner) {
            panic!("unregistered action extension owner: {expected_owner:?}");
        }
        if descriptors.is_empty() {
            panic!("action extension must not be empty");
        }
        self.validate_group(expected_owner, descriptors);
    }

    fn validate_group(
        &self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) {
        for (index, descriptor) in descriptors.iter().enumerate() {
            Self::validate_owner(expected_owner, descriptor);
            self.validate_id(descriptor.id, &descriptors[..index]);
        }
    }

    fn validate_owner(expected_owner: ActionOwnerId, descriptor: &ActionDescriptor) {
        if descriptor.id.owner != expected_owner {
            panic!("action descriptor owner mismatch: {:?}", descriptor.id);
        }
    }

    fn validate_id(&self, id: ActionId, preceding: &[ActionDescriptor]) {
        if self.has_id(id) || preceding.iter().any(|descriptor| descriptor.id == id) {
            panic!("duplicate ActionId: {id:?}");
        }
    }

    fn has_id(&self, id: ActionId) -> bool {
        self.actions
            .iter()
            .chain(&self.extensions)
            .any(|(descriptor, _)| descriptor.id == id)
    }

    fn append(target: &mut Vec<(ActionDescriptor, Value)>, descriptors: &[ActionDescriptor]) {
        target.extend(
            descriptors
                .iter()
                .map(|descriptor| (*descriptor, action(descriptor))),
        );
    }
}

fn action(descriptor: &ActionDescriptor) -> Value {
    json!({
        "id": action_id(descriptor.id),
        "label": descriptor.label,
        "input": descriptor.input.map(action_input),
        "output": descriptor.output.map(action_output),
        "error": descriptor.error.map(domain_error_id),
    })
}

fn action_input(descriptor: ActionInputDescriptor) -> Value {
    match descriptor {
        ActionInputDescriptor::Scalar(scalar) => field_projection::scalar(scalar),
        ActionInputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object_id(id) })
        }
        ActionInputDescriptor::DomainIdentity(id) => {
            json!({ "kind": "domainIdentity", "id": domain_identity_id(id) })
        }
    }
}

fn action_output(descriptor: ActionOutputDescriptor) -> Value {
    match descriptor {
        ActionOutputDescriptor::Scalar(scalar) => field_projection::scalar(scalar),
        ActionOutputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object_id(id) })
        }
        ActionOutputDescriptor::DomainEvent(id) => {
            json!({ "kind": "domainEvent", "id": domain_event_id(id) })
        }
        ActionOutputDescriptor::Optional(value) => {
            json!({ "kind": "optional", "value": action_output(*value) })
        }
        ActionOutputDescriptor::List(element) => {
            json!({ "kind": "list", "element": action_output(*element) })
        }
    }
}
