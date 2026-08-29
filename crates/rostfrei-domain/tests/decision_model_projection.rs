#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
use domain::DecisionOutcome;
use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DecisionDescriptor,
    DecisionGroupType, DecisionId, DecisionImplementationDescriptor, DecisionOutcomeDescriptor,
    DecisionOutcomeId, DecisionOutcomeShapeDescriptor, DecisionOwnerId, DomainIdentity,
    DomainModelError, Entity, ValueObject, domain_decisions, domain_model,
};
use serde_json::{Value, json};

struct AggregateProjectionDecisions;
struct EntityProjectionDecisions;

#[derive(BoundedContext)]
#[domain(id = "decision-projection", label = "Decision projection")]
struct ProjectionContext;

#[derive(DomainIdentity)]
#[domain(owner = ProjectionRoot)]
struct ProjectionIdentity(u64);

#[derive(Aggregate)]
#[domain(
    id = "projection-aggregate",
    label = "Projection aggregate",
    context = ProjectionContext,
    root = ProjectionRoot,
    decisions = [AggregateProjectionDecisions]
)]
struct ProjectionAggregate;

#[derive(Entity)]
#[domain(
    id = "projection-root",
    label = "Projection root",
    owner = ProjectionAggregate,
    decisions = [EntityProjectionDecisions]
)]
struct ProjectionRoot {
    #[domain(identity)]
    id: ProjectionIdentity,
}

#[derive(ValueObject, Clone, Copy)]
#[domain(
    id = "projection-input",
    label = "Projection input",
    owner = ProjectionAggregate
)]
struct ProjectionInput(u64);

#[derive(ValueObject)]
#[domain(
    id = "projection-output",
    label = "Projection output",
    owner = ProjectionAggregate
)]
struct ProjectionOutput(bool);

#[derive(ValueObject)]
#[domain(
    id = "projection-reason",
    label = "Projection reason",
    owner = ProjectionAggregate
)]
struct ProjectionReason;

#[derive(DecisionOutcome)]
enum ProjectionOutcome {
    #[outcome(id = "deferred", label = "Deferred")]
    Deferred,
    #[outcome(id = "accepted", label = "Accepted")]
    Accepted(ProjectionOutput, bool),
    #[outcome(id = "rejected", label = "Rejected")]
    Rejected {
        reason: ProjectionReason,
        retryable: bool,
    },
}

#[domain_decisions(aggregate, group = AggregateProjectionDecisions)]
impl ProjectionAggregate {
    #[decision(id = "evaluate", label = "Evaluate")]
    const fn evaluate(input: ProjectionInput, threshold: u64) -> ProjectionOutcome {
        if input.0 >= threshold {
            ProjectionOutcome::Accepted(ProjectionOutput(true), true)
        } else {
            ProjectionOutcome::Rejected {
                reason: ProjectionReason,
                retryable: true,
            }
        }
    }

    #[decision(id = "shared", label = "Aggregate shared")]
    const fn shared(input: ProjectionInput) -> ProjectionOutcome {
        if input.0 > 0 {
            ProjectionOutcome::Deferred
        } else {
            ProjectionOutcome::Rejected {
                reason: ProjectionReason,
                retryable: false,
            }
        }
    }
}

#[domain_decisions(entity, group = EntityProjectionDecisions)]
impl ProjectionRoot {
    #[decision(id = "shared", label = "Entity shared")]
    const fn shared(input: ProjectionInput) -> ProjectionOutcome {
        if input.0 > 0 {
            ProjectionOutcome::Accepted(ProjectionOutput(true), false)
        } else {
            ProjectionOutcome::Deferred
        }
    }
}

fn projected_model() -> Result<Value, DomainModelError> {
    domain_model! {
        contexts: [ProjectionContext],
        aggregates: [ProjectionAggregate],
        entities: [ProjectionRoot],
        identities: [ProjectionIdentity],
        value_objects: [ProjectionInput, ProjectionOutput, ProjectionReason],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn projects_outcome_shapes_and_named_parameters_as_exact_json() {
    assert_eq!(
        projected_model().expect("decision model projection should succeed")["decisions"][0],
        json!({
            "id": {
                "owner": {
                    "kind": "aggregate",
                    "id": {
                        "context": "decision-projection",
                        "local": "projection-aggregate",
                    },
                },
                "local": "evaluate",
            },
            "label": "Evaluate",
            "parameters": [
                {
                    "name": "input",
                    "input": {
                        "kind": "valueObject",
                        "id": {
                            "owner": {
                                "kind": "aggregate",
                                "id": {
                                    "context": "decision-projection",
                                    "local": "projection-aggregate",
                                },
                            },
                            "local": "projection-input",
                        },
                    },
                },
                {
                    "name": "threshold",
                    "input": { "kind": "scalar", "scalar": "u64" },
                },
            ],
            "outcomes": [
                {
                    "id": {
                        "decision": {
                            "owner": {
                                "kind": "aggregate",
                                "id": {
                                    "context": "decision-projection",
                                    "local": "projection-aggregate",
                                },
                            },
                            "local": "evaluate",
                        },
                        "local": "deferred",
                    },
                    "label": "Deferred",
                    "shape": { "kind": "unit" },
                },
                {
                    "id": {
                        "decision": {
                            "owner": {
                                "kind": "aggregate",
                                "id": {
                                    "context": "decision-projection",
                                    "local": "projection-aggregate",
                                },
                            },
                            "local": "evaluate",
                        },
                        "local": "accepted",
                    },
                    "label": "Accepted",
                    "shape": {
                        "kind": "tuple",
                        "fields": [
                            {
                                "kind": "valueObject",
                                "id": {
                                    "owner": {
                                        "kind": "aggregate",
                                        "id": {
                                            "context": "decision-projection",
                                            "local": "projection-aggregate",
                                        },
                                    },
                                    "local": "projection-output",
                                },
                            },
                            { "kind": "scalar", "scalar": "bool" },
                        ],
                    },
                },
                {
                    "id": {
                        "decision": {
                            "owner": {
                                "kind": "aggregate",
                                "id": {
                                    "context": "decision-projection",
                                    "local": "projection-aggregate",
                                },
                            },
                            "local": "evaluate",
                        },
                        "local": "rejected",
                    },
                    "label": "Rejected",
                    "shape": {
                        "kind": "struct",
                        "fields": [
                            {
                                "name": "reason",
                                "value": {
                                    "kind": "valueObject",
                                    "id": {
                                        "owner": {
                                            "kind": "aggregate",
                                            "id": {
                                                "context": "decision-projection",
                                                "local": "projection-aggregate",
                                            },
                                        },
                                        "local": "projection-reason",
                                    },
                                },
                            },
                            {
                                "name": "retryable",
                                "value": { "kind": "scalar", "scalar": "bool" },
                            },
                        ],
                    },
                },
            ],
            "implementation": { "kind": "rust" },
        })
    );
}

#[test]
fn outcome_ids_are_decision_scoped_and_owner_order_is_stable() {
    let model = projected_model().expect("decision model projection should succeed");
    let decisions = model["decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), 3);
    assert_eq!(decisions[0]["id"]["local"], "evaluate");
    assert_eq!(decisions[1]["id"]["local"], "shared");
    assert_eq!(decisions[1]["id"]["owner"]["kind"], "aggregate");
    assert_eq!(decisions[2]["id"]["local"], "shared");
    assert_eq!(decisions[2]["id"]["owner"]["kind"], "entity");
    assert_eq!(
        decisions[0]["outcomes"][0]["id"]["decision"]["local"],
        "evaluate"
    );
    assert_eq!(
        decisions[1]["outcomes"][0]["id"]["decision"]["local"],
        "shared"
    );
}

const DUPLICATE_CONTEXT_ID: BoundedContextId = BoundedContextId("duplicate-decisions");
const DUPLICATE_AGGREGATE_ID: AggregateId = AggregateId {
    context: DUPLICATE_CONTEXT_ID,
    local: "owner",
};
const DUPLICATE_ID: DecisionId = DecisionId {
    owner: DecisionOwnerId::Aggregate(DUPLICATE_AGGREGATE_ID),
    local: "same",
};
const DUPLICATE_OUTCOMES: &[DecisionOutcomeDescriptor] = &[DecisionOutcomeDescriptor {
    local_id: "done",
    label: "Done",
    shape: DecisionOutcomeShapeDescriptor::Unit,
}];
const FIRST_DUPLICATE_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DUPLICATE_ID,
    label: "First",
    parameters: &[],
    outcomes: DUPLICATE_OUTCOMES,
    implementation: DecisionImplementationDescriptor::Rust,
}];
const SECOND_DUPLICATE_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DUPLICATE_ID,
    label: "Second",
    parameters: &[],
    outcomes: DUPLICATE_OUTCOMES,
    implementation: DecisionImplementationDescriptor::Rust,
}];

struct FirstDuplicateGroup;
struct SecondDuplicateGroup;

#[derive(BoundedContext)]
#[domain(id = "duplicate-decisions", label = "Duplicate decisions")]
struct DuplicateContext;

#[derive(DomainIdentity)]
#[domain(owner = DuplicateRoot)]
struct DuplicateIdentity(u64);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = DuplicateOwner)]
struct DuplicateRoot {
    #[domain(identity)]
    id: DuplicateIdentity,
}

#[derive(Aggregate)]
#[domain(
    id = "owner",
    label = "Duplicate owner",
    context = DuplicateContext,
    root = DuplicateRoot,
    decisions = [FirstDuplicateGroup, SecondDuplicateGroup]
)]
struct DuplicateOwner;

impl DecisionGroupType for FirstDuplicateGroup {
    type Owner = DuplicateOwner;

    const DECISIONS: &'static [DecisionDescriptor] = FIRST_DUPLICATE_DECISIONS;
}

impl DecisionGroupType for SecondDuplicateGroup {
    type Owner = DuplicateOwner;

    const DECISIONS: &'static [DecisionDescriptor] = SECOND_DUPLICATE_DECISIONS;
}

#[test]
fn rejects_duplicate_decision_ids_across_groups() {
    let mut builder = DomainModelBuilder::new();
    let error = builder
        .add_aggregate_type::<DuplicateOwner>()
        .expect_err("the second group should conflict with the first group");

    assert_eq!(
        error,
        DomainModelError::DuplicateDecisionId {
            id: Box::new(DUPLICATE_ID),
        }
    );
    assert_eq!(
        error.to_string(),
        format!("duplicate DecisionId: {DUPLICATE_ID:?}")
    );
}

const EMPTY_OUTCOME_DECISION_ID: DecisionId = DecisionId {
    owner: DecisionOwnerId::Aggregate(AggregateId {
        context: DUPLICATE_CONTEXT_ID,
        local: "empty-outcome-owner",
    }),
    local: "empty",
};
const EMPTY_OUTCOME_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: EMPTY_OUTCOME_DECISION_ID,
    label: "Empty",
    parameters: &[],
    outcomes: &[],
    implementation: DecisionImplementationDescriptor::Rust,
}];

struct EmptyOutcomeGroup;

#[derive(DomainIdentity)]
#[domain(owner = EmptyOutcomeRoot)]
struct EmptyOutcomeIdentity(u64);

#[derive(Entity)]
#[domain(id = "empty-root", label = "Empty root", owner = EmptyOutcomeOwner)]
struct EmptyOutcomeRoot {
    #[domain(identity)]
    id: EmptyOutcomeIdentity,
}

#[derive(Aggregate)]
#[domain(
    id = "empty-outcome-owner",
    label = "Empty outcome owner",
    context = DuplicateContext,
    root = EmptyOutcomeRoot,
    decisions = [EmptyOutcomeGroup]
)]
struct EmptyOutcomeOwner;

impl DecisionGroupType for EmptyOutcomeGroup {
    type Owner = EmptyOutcomeOwner;

    const DECISIONS: &'static [DecisionDescriptor] = EMPTY_OUTCOME_DECISIONS;
}

const DUPLICATE_OUTCOME_DECISION_ID: DecisionId = DecisionId {
    owner: DecisionOwnerId::Aggregate(AggregateId {
        context: DUPLICATE_CONTEXT_ID,
        local: "duplicate-outcome-owner",
    }),
    local: "duplicate",
};
const DUPLICATE_LOCAL_OUTCOMES: &[DecisionOutcomeDescriptor] = &[
    DecisionOutcomeDescriptor {
        local_id: "same",
        label: "First",
        shape: DecisionOutcomeShapeDescriptor::Unit,
    },
    DecisionOutcomeDescriptor {
        local_id: "same",
        label: "Second",
        shape: DecisionOutcomeShapeDescriptor::Unit,
    },
];
const DUPLICATE_OUTCOME_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DUPLICATE_OUTCOME_DECISION_ID,
    label: "Duplicate",
    parameters: &[],
    outcomes: DUPLICATE_LOCAL_OUTCOMES,
    implementation: DecisionImplementationDescriptor::Rust,
}];

struct DuplicateOutcomeGroup;

#[derive(DomainIdentity)]
#[domain(owner = DuplicateOutcomeRoot)]
struct DuplicateOutcomeIdentity(u64);

#[derive(Entity)]
#[domain(
    id = "duplicate-root",
    label = "Duplicate root",
    owner = DuplicateOutcomeOwner
)]
struct DuplicateOutcomeRoot {
    #[domain(identity)]
    id: DuplicateOutcomeIdentity,
}

#[derive(Aggregate)]
#[domain(
    id = "duplicate-outcome-owner",
    label = "Duplicate outcome owner",
    context = DuplicateContext,
    root = DuplicateOutcomeRoot,
    decisions = [DuplicateOutcomeGroup]
)]
struct DuplicateOutcomeOwner;

impl DecisionGroupType for DuplicateOutcomeGroup {
    type Owner = DuplicateOutcomeOwner;

    const DECISIONS: &'static [DecisionDescriptor] = DUPLICATE_OUTCOME_DECISIONS;
}

#[test]
fn rejects_a_manual_decision_descriptor_without_outcomes() {
    let mut builder = DomainModelBuilder::new();
    let error = builder
        .add_aggregate_type::<EmptyOutcomeOwner>()
        .expect_err("a manual decision without outcomes should be rejected");

    assert_eq!(
        error,
        DomainModelError::DecisionWithoutOutcomes {
            decision_id: Box::new(EMPTY_OUTCOME_DECISION_ID),
        }
    );
    assert_eq!(
        error.to_string(),
        format!("decision must declare at least one active outcome: {EMPTY_OUTCOME_DECISION_ID:?}")
    );
}

#[test]
fn rejects_duplicate_outcome_ids_in_a_manual_decision_group() {
    let mut builder = DomainModelBuilder::new();
    let error = builder
        .add_aggregate_type::<DuplicateOutcomeOwner>()
        .expect_err("duplicate outcome IDs in a manual group should be rejected");
    let duplicate_id = DecisionOutcomeId {
        decision: DUPLICATE_OUTCOME_DECISION_ID,
        local: "same",
    };

    assert_eq!(
        error,
        DomainModelError::DuplicateDecisionOutcomeId {
            id: Box::new(duplicate_id),
        }
    );
    assert_eq!(
        error.to_string(),
        format!("duplicate DecisionOutcomeId: {duplicate_id:?}")
    );
}
