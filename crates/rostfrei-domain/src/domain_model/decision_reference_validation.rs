use crate::{
    DecisionDescriptor, DecisionId, DecisionInputDescriptor, DecisionOutcomeShapeDescriptor,
    DecisionOutcomeValueDescriptor, ValueObjectId,
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
    for (parameter_index, parameter) in descriptor.parameters.iter().enumerate() {
        let location = format!("parameters[{parameter_index}].input");
        validate_input_reference(descriptor.id, parameter.input, &location, inventory)?;
    }
    for (outcome_index, outcome) in descriptor.outcomes.iter().enumerate() {
        match outcome.shape {
            DecisionOutcomeShapeDescriptor::Unit => {}
            DecisionOutcomeShapeDescriptor::Tuple { fields } => {
                for (field_index, value) in fields.iter().enumerate() {
                    let location = format!("outcomes[{outcome_index}].shape.fields[{field_index}]");
                    validate_outcome_value_reference(descriptor.id, *value, &location, inventory)?;
                }
            }
            DecisionOutcomeShapeDescriptor::Struct { fields } => {
                for (field_index, field) in fields.iter().enumerate() {
                    let location =
                        format!("outcomes[{outcome_index}].shape.fields[{field_index}].value");
                    validate_outcome_value_reference(
                        descriptor.id,
                        field.value,
                        &location,
                        inventory,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_input_reference(
    decision_id: DecisionId,
    input: DecisionInputDescriptor,
    location: &str,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    match input {
        DecisionInputDescriptor::Scalar(_) => Ok(()),
        DecisionInputDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, location, inventory)
        }
    }
}

fn validate_outcome_value_reference(
    decision_id: DecisionId,
    value: DecisionOutcomeValueDescriptor,
    location: &str,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    match value {
        DecisionOutcomeValueDescriptor::Scalar(_) => Ok(()),
        DecisionOutcomeValueDescriptor::ValueObject(id) => {
            validate_value_object_reference(decision_id, id, location, inventory)
        }
    }
}

fn validate_value_object_reference(
    decision_id: DecisionId,
    value_object_id: ValueObjectId,
    location: &str,
    inventory: &DecisionReferenceInventory,
) -> Result<(), DomainModelError> {
    if !inventory.value_objects.contains(&value_object_id) {
        return Err(DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(decision_id),
            value_object_id: Box::new(value_object_id),
            location: location.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        AggregateId, BoundedContextId, DecisionImplementationDescriptor, DecisionOutcomeDescriptor,
        DecisionOwnerId, DecisionParameterDescriptor, ScalarType, ValueObjectOwnerId,
    };

    use super::*;

    const AGGREGATE_ID: AggregateId = AggregateId {
        context: BoundedContextId("validation-test"),
        local: "owner",
    };
    const DECISION_ID: DecisionId = DecisionId {
        owner: DecisionOwnerId::Aggregate(AGGREGATE_ID),
        local: "decide",
    };
    const PARAMETER_VALUE_OBJECT_ID: ValueObjectId = ValueObjectId {
        owner: ValueObjectOwnerId::Aggregate(AGGREGATE_ID),
        local: "parameter",
    };
    const OUTCOME_VALUE_OBJECT_ID: ValueObjectId = ValueObjectId {
        owner: ValueObjectOwnerId::Aggregate(AGGREGATE_ID),
        local: "outcome",
    };
    const DESCRIPTOR: DecisionDescriptor = DecisionDescriptor {
        id: DECISION_ID,
        label: "Decide",
        parameters: &[DecisionParameterDescriptor {
            name: "request",
            input: DecisionInputDescriptor::ValueObject(PARAMETER_VALUE_OBJECT_ID),
        }],
        outcomes: &[DecisionOutcomeDescriptor {
            local_id: "completed",
            label: "Completed",
            shape: DecisionOutcomeShapeDescriptor::Tuple {
                fields: &[
                    DecisionOutcomeValueDescriptor::Scalar(ScalarType::Bool),
                    DecisionOutcomeValueDescriptor::ValueObject(OUTCOME_VALUE_OBJECT_ID),
                ],
            },
        }],
        implementation: DecisionImplementationDescriptor::Rust,
    };

    #[test]
    fn validates_parameters_then_ordered_outcome_fields_with_stable_locations() {
        assert_eq!(
            validate([DESCRIPTOR], &DecisionReferenceInventory::new(vec![])),
            Err(DomainModelError::DecisionReferenceInventoryViolation {
                decision_id: Box::new(DECISION_ID),
                value_object_id: Box::new(PARAMETER_VALUE_OBJECT_ID),
                location: "parameters[0].input".to_owned(),
            })
        );
        assert_eq!(
            validate(
                [DESCRIPTOR],
                &DecisionReferenceInventory::new(vec![PARAMETER_VALUE_OBJECT_ID]),
            ),
            Err(DomainModelError::DecisionReferenceInventoryViolation {
                decision_id: Box::new(DECISION_ID),
                value_object_id: Box::new(OUTCOME_VALUE_OBJECT_ID),
                location: "outcomes[0].shape.fields[1]".to_owned(),
            })
        );
    }
}
