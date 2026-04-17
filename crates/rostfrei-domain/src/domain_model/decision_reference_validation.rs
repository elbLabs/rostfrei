use crate::{
    DecisionDescriptor, DecisionId, DecisionInputDescriptor, DecisionOutputDescriptor,
    ValueObjectId,
};

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
) {
    for descriptor in descriptors {
        validate_references(descriptor, inventory);
    }
}

fn validate_references(descriptor: DecisionDescriptor, inventory: &DecisionReferenceInventory) {
    validate_input_reference(descriptor.id, descriptor.input, inventory);
    validate_output_reference(descriptor.id, descriptor.output, inventory);
}

fn validate_input_reference(
    decision_id: DecisionId,
    input: DecisionInputDescriptor,
    inventory: &DecisionReferenceInventory,
) {
    match input {
        DecisionInputDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, "input", inventory);
        }
    }
}

fn validate_output_reference(
    decision_id: DecisionId,
    output: DecisionOutputDescriptor,
    inventory: &DecisionReferenceInventory,
) {
    match output {
        DecisionOutputDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, "output", inventory);
        }
    }
}

fn validate_value_object_reference(
    decision_id: DecisionId,
    value_object_id: ValueObjectId,
    location: &str,
    inventory: &DecisionReferenceInventory,
) {
    if !inventory.value_objects.contains(&value_object_id) {
        missing_reference(decision_id, value_object_id, location);
    }
}

fn missing_reference(decision_id: DecisionId, value_object_id: ValueObjectId, location: &str) -> ! {
    panic!(
        "Decision reference inventory violation: decision {decision_id:?} references missing {value_object_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `value_objects`"
    );
}
