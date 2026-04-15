use serde_json::{Value, json};

use crate::{
    DecisionDescriptor, DecisionId, DecisionImplementationDescriptor, DecisionInputDescriptor,
    DecisionOutputDescriptor, DecisionOwnerId,
};

use super::{
    decision_reference_validation::{self, DecisionReferenceInventory},
    error::DomainModelError,
    id_projection::{decision_owner as decision_owner_id, value_object as value_object_id},
};

pub(super) struct DecisionProjection {
    registered_owners: Vec<DecisionOwnerId>,
    decisions: Vec<(DecisionDescriptor, Value)>,
}

impl DecisionProjection {
    pub(super) const fn new() -> Self {
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
    ) -> Result<(), DomainModelError> {
        self.validate_registered_owner(expected_owner)?;
        self.validate_group(expected_owner, descriptors)?;
        self.decisions.extend(
            descriptors
                .iter()
                .map(|descriptor| (*descriptor, decision(*descriptor))),
        );
        Ok(())
    }

    pub(super) fn validate_references(
        &self,
        inventory: &DecisionReferenceInventory,
    ) -> Result<(), DomainModelError> {
        decision_reference_validation::validate(
            self.decisions.iter().map(|(descriptor, _)| *descriptor),
            inventory,
        )
    }

    pub(super) fn into_values(self) -> Vec<Value> {
        self.decisions.into_iter().map(|(_, value)| value).collect()
    }

    fn validate_registered_owner(&self, owner: DecisionOwnerId) -> Result<(), DomainModelError> {
        if !self.registered_owners.contains(&owner) {
            return Err(DomainModelError::UnregisteredDecisionOwner {
                owner: Box::new(owner),
            });
        }
        Ok(())
    }

    fn validate_group(
        &self,
        expected_owner: DecisionOwnerId,
        descriptors: &'static [DecisionDescriptor],
    ) -> Result<(), DomainModelError> {
        for (index, descriptor) in descriptors.iter().enumerate() {
            Self::validate_owner(expected_owner, descriptor)?;
            self.validate_id(descriptor.id, descriptors.iter().take(index))?;
        }
        Ok(())
    }

    fn validate_owner(
        expected_owner: DecisionOwnerId,
        descriptor: &DecisionDescriptor,
    ) -> Result<(), DomainModelError> {
        if descriptor.id.owner != expected_owner {
            return Err(DomainModelError::DecisionDescriptorOwnerMismatch {
                id: Box::new(descriptor.id),
            });
        }
        Ok(())
    }

    fn validate_id<'a>(
        &self,
        id: DecisionId,
        preceding: impl Iterator<Item = &'a DecisionDescriptor>,
    ) -> Result<(), DomainModelError> {
        if self.has_id(id) || preceding.into_iter().any(|descriptor| descriptor.id == id) {
            return Err(DomainModelError::DuplicateDecisionId { id: Box::new(id) });
        }
        Ok(())
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
        "parameters": descriptor.parameters.iter().map(|parameter| json!({
            "name": parameter.name,
            "input": decision_input(parameter.input),
        })).collect::<Vec<_>>(),
        "output": descriptor.output.map(decision_output),
        "error": descriptor.error.map(decision_output),
        "implementation": decision_implementation(descriptor.implementation),
    })
}

fn decision_input(descriptor: DecisionInputDescriptor) -> Value {
    match descriptor {
        DecisionInputDescriptor::Scalar(scalar) => super::field_projection::scalar(scalar),
        DecisionInputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object_id(id) })
        }
    }
}

fn decision_output(descriptor: DecisionOutputDescriptor) -> Value {
    match descriptor {
        DecisionOutputDescriptor::Scalar(scalar) => super::field_projection::scalar(scalar),
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
