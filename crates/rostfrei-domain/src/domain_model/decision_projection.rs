use serde_json::{Value, json};

use crate::{
    DecisionDescriptor, DecisionId, DecisionImplementationDescriptor, DecisionInputDescriptor,
    DecisionOutcomeDescriptor, DecisionOutcomeId, DecisionOutcomeNamedFieldDescriptor,
    DecisionOutcomeShapeDescriptor, DecisionOutcomeValueDescriptor, DecisionOwnerId,
};

use super::{
    decision_reference_validation::{self, DecisionReferenceInventory},
    error::DomainModelError,
    field_projection,
    id_projection::{decision as decision_id, decision_outcome as decision_outcome_id},
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
            Self::validate_outcomes(descriptor)?;
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

    fn validate_outcomes(descriptor: &DecisionDescriptor) -> Result<(), DomainModelError> {
        if descriptor.outcomes.is_empty() {
            return Err(DomainModelError::DecisionWithoutOutcomes {
                decision_id: Box::new(descriptor.id),
            });
        }
        for (index, outcome) in descriptor.outcomes.iter().enumerate() {
            if descriptor
                .outcomes
                .iter()
                .take(index)
                .any(|preceding| preceding.local_id == outcome.local_id)
            {
                return Err(DomainModelError::DuplicateDecisionOutcomeId {
                    id: Box::new(DecisionOutcomeId {
                        decision: descriptor.id,
                        local: outcome.local_id,
                    }),
                });
            }
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
        "id": decision_id(descriptor.id),
        "label": descriptor.label,
        "parameters": descriptor.parameters.iter().map(|parameter| json!({
            "name": parameter.name,
            "input": decision_input(parameter.input),
        })).collect::<Vec<_>>(),
        "outcomes": descriptor.outcomes.iter().map(|outcome| {
            decision_outcome(descriptor.id, outcome)
        }).collect::<Vec<_>>(),
        "implementation": decision_implementation(descriptor.implementation),
    })
}

fn decision_input(descriptor: DecisionInputDescriptor) -> Value {
    match descriptor {
        DecisionInputDescriptor::Scalar(scalar) => field_projection::scalar(scalar),
        DecisionInputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": super::id_projection::value_object(id) })
        }
    }
}

fn decision_outcome(decision: DecisionId, descriptor: &DecisionOutcomeDescriptor) -> Value {
    json!({
        "id": decision_outcome_id(DecisionOutcomeId {
            decision,
            local: descriptor.local_id,
        }),
        "label": descriptor.label,
        "shape": decision_outcome_shape(descriptor.shape),
    })
}

fn decision_outcome_shape(descriptor: DecisionOutcomeShapeDescriptor) -> Value {
    match descriptor {
        DecisionOutcomeShapeDescriptor::Unit => json!({ "kind": "unit" }),
        DecisionOutcomeShapeDescriptor::Tuple { fields } => json!({
            "kind": "tuple",
            "fields": fields.iter().copied().map(decision_outcome_value).collect::<Vec<_>>(),
        }),
        DecisionOutcomeShapeDescriptor::Struct { fields } => json!({
            "kind": "struct",
            "fields": fields.iter().map(decision_outcome_field).collect::<Vec<_>>(),
        }),
    }
}

fn decision_outcome_field(descriptor: &DecisionOutcomeNamedFieldDescriptor) -> Value {
    json!({
        "name": descriptor.name,
        "value": decision_outcome_value(descriptor.value),
    })
}

fn decision_outcome_value(descriptor: DecisionOutcomeValueDescriptor) -> Value {
    match descriptor {
        DecisionOutcomeValueDescriptor::Scalar(scalar) => field_projection::scalar(scalar),
        DecisionOutcomeValueDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": super::id_projection::value_object(id) })
        }
    }
}

fn decision_implementation(descriptor: DecisionImplementationDescriptor) -> Value {
    match descriptor {
        DecisionImplementationDescriptor::Rust => json!({ "kind": "rust" }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        AggregateId, BoundedContextId, DecisionGroupType, DecisionOwnerId, DecisionOwnerType,
        ScalarType, ValueObjectId, ValueObjectOwnerId,
    };

    use super::*;

    const AGGREGATE_ID: AggregateId = AggregateId {
        context: BoundedContextId("projection-test"),
        local: "owner",
    };
    const DECISION_ID: DecisionId = DecisionId {
        owner: DecisionOwnerId::Aggregate(AGGREGATE_ID),
        local: "route",
    };
    const VALUE_OBJECT_ID: ValueObjectId = ValueObjectId {
        owner: ValueObjectOwnerId::Aggregate(AGGREGATE_ID),
        local: "reason",
    };
    struct ManualOwner;

    impl DecisionOwnerType for ManualOwner {
        const DECISION_OWNER_ID: DecisionOwnerId = DecisionOwnerId::Aggregate(AGGREGATE_ID);
    }

    struct NoActiveOutcomes;

    impl DecisionGroupType for NoActiveOutcomes {
        type Owner = ManualOwner;

        const DECISIONS: &'static [DecisionDescriptor] = &[DecisionDescriptor {
            id: DECISION_ID,
            label: "Route",
            parameters: &[],
            outcomes: &[],
            implementation: DecisionImplementationDescriptor::Rust,
        }];
    }

    struct DuplicateOutcomes;

    impl DecisionGroupType for DuplicateOutcomes {
        type Owner = ManualOwner;

        const DECISIONS: &'static [DecisionDescriptor] = &[DecisionDescriptor {
            id: DECISION_ID,
            label: "Route",
            parameters: &[],
            outcomes: &[
                DecisionOutcomeDescriptor {
                    local_id: "queued",
                    label: "Queued",
                    shape: DecisionOutcomeShapeDescriptor::Unit,
                },
                DecisionOutcomeDescriptor {
                    local_id: "completed",
                    label: "Completed",
                    shape: DecisionOutcomeShapeDescriptor::Unit,
                },
                DecisionOutcomeDescriptor {
                    local_id: "queued",
                    label: "Queued again",
                    shape: DecisionOutcomeShapeDescriptor::Unit,
                },
                DecisionOutcomeDescriptor {
                    local_id: "completed",
                    label: "Completed again",
                    shape: DecisionOutcomeShapeDescriptor::Unit,
                },
            ],
            implementation: DecisionImplementationDescriptor::Rust,
        }];
    }

    const OUTCOMES: &[DecisionOutcomeDescriptor] = &[
        DecisionOutcomeDescriptor {
            local_id: "queued",
            label: "Queued",
            shape: DecisionOutcomeShapeDescriptor::Unit,
        },
        DecisionOutcomeDescriptor {
            local_id: "redirected",
            label: "Redirected",
            shape: DecisionOutcomeShapeDescriptor::Tuple {
                fields: &[
                    DecisionOutcomeValueDescriptor::Scalar(ScalarType::U16),
                    DecisionOutcomeValueDescriptor::ValueObject(VALUE_OBJECT_ID),
                ],
            },
        },
        DecisionOutcomeDescriptor {
            local_id: "completed",
            label: "Completed",
            shape: DecisionOutcomeShapeDescriptor::Struct {
                fields: &[DecisionOutcomeNamedFieldDescriptor {
                    name: "attempts",
                    value: DecisionOutcomeValueDescriptor::Scalar(ScalarType::U8),
                }],
            },
        },
    ];

    #[test]
    fn rejects_manual_decision_with_no_active_outcomes() {
        let mut projection = DecisionProjection::new();
        projection.register_owner(ManualOwner::DECISION_OWNER_ID);

        let Err(error) =
            projection.add_group(ManualOwner::DECISION_OWNER_ID, NoActiveOutcomes::DECISIONS)
        else {
            panic!("decision without active outcomes must be rejected");
        };
        let expected = DomainModelError::DecisionWithoutOutcomes {
            decision_id: Box::new(DECISION_ID),
        };

        assert_eq!(error, expected);
        assert_eq!(
            error.to_string(),
            format!("decision must declare at least one active outcome: {DECISION_ID:?}")
        );
        assert!(projection.into_values().is_empty());
    }

    #[test]
    fn rejects_first_duplicate_outcome_local_id_in_source_order() {
        let mut projection = DecisionProjection::new();
        projection.register_owner(ManualOwner::DECISION_OWNER_ID);

        let Err(error) =
            projection.add_group(ManualOwner::DECISION_OWNER_ID, DuplicateOutcomes::DECISIONS)
        else {
            panic!("duplicate outcome local IDs must be rejected");
        };
        let duplicate_id = DecisionOutcomeId {
            decision: DECISION_ID,
            local: "queued",
        };
        let expected = DomainModelError::DuplicateDecisionOutcomeId {
            id: Box::new(duplicate_id),
        };

        assert_eq!(error, expected);
        assert_eq!(
            error.to_string(),
            format!("duplicate DecisionOutcomeId: {duplicate_id:?}")
        );
        assert!(projection.into_values().is_empty());
    }

    #[test]
    fn projects_ordered_outcomes_with_scoped_ids_and_explicit_shapes() {
        let value = decision(DecisionDescriptor {
            id: DECISION_ID,
            label: "Route",
            parameters: &[],
            outcomes: OUTCOMES,
            implementation: DecisionImplementationDescriptor::Rust,
        });

        assert!(value.get("output").is_none());
        assert!(value.get("error").is_none());
        assert!(value.get("schemaVersion").is_none());
        assert_eq!(
            value["outcomes"],
            json!([
                {
                    "id": {
                        "decision": {
                            "owner": {
                                "kind": "aggregate",
                                "id": { "context": "projection-test", "local": "owner" },
                            },
                            "local": "route",
                        },
                        "local": "queued",
                    },
                    "label": "Queued",
                    "shape": { "kind": "unit" },
                },
                {
                    "id": {
                        "decision": {
                            "owner": {
                                "kind": "aggregate",
                                "id": { "context": "projection-test", "local": "owner" },
                            },
                            "local": "route",
                        },
                        "local": "redirected",
                    },
                    "label": "Redirected",
                    "shape": {
                        "kind": "tuple",
                        "fields": [
                            { "kind": "scalar", "scalar": "u16" },
                            {
                                "kind": "valueObject",
                                "id": {
                                    "owner": {
                                        "kind": "aggregate",
                                        "id": {
                                            "context": "projection-test",
                                            "local": "owner",
                                        },
                                    },
                                    "local": "reason",
                                },
                            },
                        ],
                    },
                },
                {
                    "id": {
                        "decision": {
                            "owner": {
                                "kind": "aggregate",
                                "id": { "context": "projection-test", "local": "owner" },
                            },
                            "local": "route",
                        },
                        "local": "completed",
                    },
                    "label": "Completed",
                    "shape": {
                        "kind": "struct",
                        "fields": [{
                            "name": "attempts",
                            "value": { "kind": "scalar", "scalar": "u8" },
                        }],
                    },
                },
            ])
        );
    }
}
