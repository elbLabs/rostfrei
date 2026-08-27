use serde_json::{Value, json};

use crate::{
    DecisionDescriptor, DecisionId, DecisionImplementationDescriptor, DecisionInputDescriptor,
    DecisionOutputDescriptor, DecisionOwnerId,
};

use super::{
    decision_reference_validation::{self, DecisionReferenceInventory},
    id_projection::{decision_owner as decision_owner_id, value_object as value_object_id},
};

pub(super) struct DecisionProjection {
    registered_owners: Vec<DecisionOwnerId>,
    decisions: Vec<(DecisionDescriptor, Value)>,
}

impl DecisionProjection {
    pub(super) fn new() -> Self {
        Self {
            registered_owners: Vec::new(),
            decisions: Vec::new(),
        }
    }

    pub(super) fn register_owner(&mut self, owner: DecisionOwnerId) {
        if !self.registered_owners.contains(&owner) {
            self.registered_owners.push(owner);
        }
    }

    pub(super) fn add_group(
        &mut self,
        expected_owner: DecisionOwnerId,
        descriptors: &'static [DecisionDescriptor],
    ) {
        self.validate_registered_owner(expected_owner);
        self.validate_group(expected_owner, descriptors);
        self.decisions.extend(
            descriptors
                .iter()
                .map(|descriptor| (*descriptor, decision(*descriptor))),
        );
    }

    pub(super) fn validate_references(&self, inventory: &DecisionReferenceInventory) {
        decision_reference_validation::validate(
            self.decisions.iter().map(|(descriptor, _)| *descriptor),
            inventory,
        );
    }

    pub(super) fn into_values(self) -> Vec<Value> {
        self.decisions.into_iter().map(|(_, value)| value).collect()
    }

    fn validate_registered_owner(&self, owner: DecisionOwnerId) {
        if !self.registered_owners.contains(&owner) {
            panic!("unregistered decision owner: {owner:?}");
        }
    }

    fn validate_group(
        &self,
        expected_owner: DecisionOwnerId,
        descriptors: &'static [DecisionDescriptor],
    ) {
        for (index, descriptor) in descriptors.iter().enumerate() {
            Self::validate_owner(expected_owner, descriptor);
            self.validate_id(descriptor.id, &descriptors[..index]);
        }
    }

    fn validate_owner(expected_owner: DecisionOwnerId, descriptor: &DecisionDescriptor) {
        if descriptor.id.owner != expected_owner {
            panic!("decision descriptor owner mismatch: {:?}", descriptor.id);
        }
    }

    fn validate_id(&self, id: DecisionId, preceding: &[DecisionDescriptor]) {
        if self.has_id(id) || preceding.iter().any(|descriptor| descriptor.id == id) {
            panic!("duplicate DecisionId: {id:?}");
        }
    }

    fn has_id(&self, id: DecisionId) -> bool {
        self.decisions
            .iter()
            .any(|(descriptor, _)| descriptor.id == id)
    }
}

fn decision(descriptor: DecisionDescriptor) -> Value {
    json!({
        "id": {
            "owner": decision_owner_id(descriptor.id.owner),
            "local": descriptor.id.local,
        },
        "label": descriptor.label,
        "input": decision_input(descriptor.input),
        "output": decision_output(descriptor.output),
        "implementation": decision_implementation(descriptor.implementation),
    })
}

fn decision_input(descriptor: DecisionInputDescriptor) -> Value {
    match descriptor {
        DecisionInputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object_id(id) })
        }
    }
}

fn decision_output(descriptor: DecisionOutputDescriptor) -> Value {
    match descriptor {
        DecisionOutputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object_id(id) })
        }
    }
}

fn decision_implementation(descriptor: DecisionImplementationDescriptor) -> Value {
    match descriptor {
        DecisionImplementationDescriptor::Rust => json!({ "kind": "rust" }),
    }
}
