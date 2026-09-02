use serde_json::{Value, json};

use crate::{
    DecisionDescriptor, DecisionImplementationDescriptor, DecisionOutcomeId, DecisionOwnerId,
};

use super::{
    error::DomainModelError,
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
        if !self.registered_owners.contains(&expected_owner) {
            return Err(DomainModelError::UnregisteredDecisionOwner {
                owner: Box::new(expected_owner),
            });
        }
        for (index, descriptor) in descriptors.iter().enumerate() {
            if descriptor.id.owner != expected_owner {
                return Err(DomainModelError::DecisionDescriptorOwnerMismatch {
                    id: Box::new(descriptor.id),
                });
            }
            if self
                .decisions
                .iter()
                .any(|(other, _)| other.id == descriptor.id)
                || descriptors
                    .iter()
                    .take(index)
                    .any(|other| other.id == descriptor.id)
            {
                return Err(DomainModelError::DuplicateDecisionId {
                    id: Box::new(descriptor.id),
                });
            }
            if descriptor.outcomes.is_empty() {
                return Err(DomainModelError::DecisionWithoutOutcomes {
                    decision_id: Box::new(descriptor.id),
                });
            }
            for (outcome_index, outcome) in descriptor.outcomes.iter().enumerate() {
                if descriptor
                    .outcomes
                    .iter()
                    .take(outcome_index)
                    .any(|other| other.local_id == outcome.local_id)
                {
                    return Err(DomainModelError::DuplicateDecisionOutcomeId {
                        id: Box::new(DecisionOutcomeId {
                            decision: descriptor.id,
                            local: outcome.local_id,
                        }),
                    });
                }
            }
        }
        self.decisions.extend(
            descriptors
                .iter()
                .map(|descriptor| (*descriptor, decision(*descriptor))),
        );
        Ok(())
    }

    pub(super) fn into_values(self) -> Vec<Value> {
        self.decisions.into_iter().map(|(_, value)| value).collect()
    }
}

fn decision(descriptor: DecisionDescriptor) -> Value {
    json!({
        "id": decision_id(descriptor.id),
        "label": descriptor.label,
        "outcomes": descriptor.outcomes.iter().map(|outcome| json!({
            "id": decision_outcome_id(DecisionOutcomeId {
                decision: descriptor.id,
                local: outcome.local_id,
            }),
            "label": outcome.label,
        })).collect::<Vec<_>>(),
        "implementation": match descriptor.implementation {
            DecisionImplementationDescriptor::Rust => json!({ "kind": "rust" }),
        },
    })
}

#[cfg(test)]
mod tests {
    use crate::{AggregateId, BoundedContextId, DecisionId, DecisionOutcomeDescriptor};

    use super::*;

    const ID: DecisionId = DecisionId {
        owner: DecisionOwnerId::Aggregate(AggregateId {
            context: BoundedContextId("projection-test"),
            local: "owner",
        }),
        local: "route",
    };

    #[test]
    fn projects_only_semantic_decision_and_outcome_metadata() {
        let value = decision(DecisionDescriptor {
            id: ID,
            label: "Route",
            outcomes: &[DecisionOutcomeDescriptor {
                local_id: "queued",
                label: "Queued",
            }],
            implementation: DecisionImplementationDescriptor::Rust,
        });

        assert!(value.get("parameters").is_none());
        assert!(value["outcomes"][0].get("shape").is_none());
        assert_eq!(value["outcomes"][0]["label"], "Queued");
    }
}
