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
    for parameter in descriptor.parameters {
        validate_input_reference(descriptor.id, parameter.input, inventory)?;
    }
    if let Some(output) = descriptor.output {
        validate_output_reference(descriptor.id, output, "output", inventory)?;
    }
    if let Some(error) = descriptor.error {
        validate_output_reference(descriptor.id, error, "error", inventory)?;
    }
    Ok(())
}

fn validate_input_reference(
    decision_id: DecisionId,
    input: DecisionInputDescriptor,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    match input {
        DecisionInputDescriptor::Scalar(_) => Ok(()),
        DecisionInputDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, "input", inventory)
        }
    }
}

fn validate_output_reference(
    decision_id: DecisionId,
    output: DecisionOutputDescriptor,
    location: &'static str,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    match output {
        DecisionOutputDescriptor::Scalar(_) => Ok(()),
        DecisionOutputDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, location, inventory)
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
