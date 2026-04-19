use crate::{
    DecisionDescriptor, DecisionId, DecisionInputDescriptor, DecisionOutputDescriptor,
    ValueObjectId,
};

use super::error::DomainModelError;

pub(super) struct DecisionReferenceInventory {
    value_objects: Vec<ValueObjectId>,
}

impl DecisionReferenceInventory {
    pub(super) const fn new(value_objects: Vec<ValueObjectId>) -> Self {
        Self { value_objects }
    }
}

pub(super) fn validate(
    descriptors: impl IntoIterator<Item = DecisionDescriptor>,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    for descriptor in descriptors {
        validate_references(descriptor, inventory)?;
    }
    Ok(())
}

fn validate_references(
    descriptor: DecisionDescriptor,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    validate_input_reference(descriptor.id, descriptor.input, inventory)?;
    validate_output_reference(descriptor.id, descriptor.output, inventory)
}

fn validate_input_reference(
    decision_id: DecisionId,
    input: DecisionInputDescriptor,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    match input {
        DecisionInputDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, "input", inventory)
        }
    }
}

fn validate_output_reference(
    decision_id: DecisionId,
    output: DecisionOutputDescriptor,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    match output {
        DecisionOutputDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, "output", inventory)
        }
    }
}

fn validate_value_object_reference(
    decision_id: DecisionId,
    value_object_id: ValueObjectId,
    location: &'static str,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    if !inventory.value_objects.contains(&value_object_id) {
        return Err(DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(decision_id),
            value_object_id: Box::new(value_object_id),
            location,
        });
    }
    Ok(())
}
