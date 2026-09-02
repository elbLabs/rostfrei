#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
use domain::DecisionOutcome;
use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DecisionDescriptor,
    DecisionGroupType, DecisionId, DecisionImplementationDescriptor, DecisionOutcomeDescriptor,
    DecisionOwnerId, DomainIdentity, DomainModelError, Entity, ValueObject, domain_decisions,
    domain_model,
};
use serde_json::Value;

struct AggregateProjectionDecisions;
struct EntityProjectionDecisions;

#[derive(BoundedContext)]
#[domain(id = "decision-projection", label = "Decision projection")]
struct ProjectionContext;

#[derive(DomainIdentity)]
struct ProjectionIdentity(u64);

#[derive(Aggregate)]
#[domain(id = "projection-aggregate", label = "Projection aggregate")]
struct ProjectionAggregate;

impl domain::AggregateDefinition for ProjectionAggregate {
    type Context = ProjectionContext;
    type Root = ProjectionRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Entity)]
#[domain(id = "projection-root", label = "Projection root")]
struct ProjectionRoot {
    #[domain(identity)]
    id: ProjectionIdentity,
}

impl domain::EntityDefinition for ProjectionRoot {
    type Owner = ProjectionAggregate;
    type Identity = ProjectionIdentity;
}

#[derive(ValueObject, Clone, Copy)]
#[domain(id = "projection-input", label = "Projection input")]
struct ProjectionInput(u64);

#[derive(ValueObject)]
#[domain(id = "projection-output", label = "Projection output")]
struct ProjectionOutput(bool);

#[derive(ValueObject)]
#[domain(id = "projection-reason", label = "Projection reason")]
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
        value_objects: [ProjectionInput, ProjectionOutput, ProjectionReason],
        services: [],
        errors: [],
        query_groups: [],
    }
}

#[test]
fn entity_decisions_are_not_implicitly_projected() {
    let model = projected_model().expect("decision model projection should succeed");
    let decisions = model["decisions"].as_array().unwrap();
    assert!(decisions.is_empty());
}

#[test]
fn entity_outcome_ids_remain_decision_scoped() {
    let descriptor = <EntityProjectionDecisions as DecisionGroupType>::DECISIONS[0];
    assert_eq!(descriptor.outcomes[0].local_id, "deferred");
    assert_eq!(descriptor.id.local, "shared");
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
}];
const FIRST_DUPLICATE_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DUPLICATE_ID,
    label: "First",
    outcomes: DUPLICATE_OUTCOMES,
    implementation: DecisionImplementationDescriptor::Rust,
}];
const SECOND_DUPLICATE_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DUPLICATE_ID,
    label: "Second",
    outcomes: DUPLICATE_OUTCOMES,
    implementation: DecisionImplementationDescriptor::Rust,
}];

struct FirstDuplicateGroup;
struct SecondDuplicateGroup;

#[derive(BoundedContext)]
#[domain(id = "duplicate-decisions", label = "Duplicate decisions")]
struct DuplicateContext;

#[derive(DomainIdentity)]
struct DuplicateIdentity(u64);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct DuplicateRoot {
    #[domain(identity)]
    id: DuplicateIdentity,
}

impl domain::EntityDefinition for DuplicateRoot {
    type Owner = DuplicateOwner;
    type Identity = DuplicateIdentity;
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Duplicate owner")]
struct DuplicateOwner;

impl domain::AggregateDefinition for DuplicateOwner {
    type Context = DuplicateContext;
    type Root = DuplicateRoot;
    type Event = domain::NoDomainEvents;
}

impl DecisionGroupType for FirstDuplicateGroup {
    type Owner = DuplicateOwner;

    const DECISIONS: &'static [DecisionDescriptor] = FIRST_DUPLICATE_DECISIONS;
}

impl DecisionGroupType for SecondDuplicateGroup {
    type Owner = DuplicateOwner;

    const DECISIONS: &'static [DecisionDescriptor] = SECOND_DUPLICATE_DECISIONS;
}

#[test]
fn unattached_duplicate_aggregate_decision_groups_are_not_registered() {
    let mut builder = DomainModelBuilder::new();
    builder.add_aggregate_type::<DuplicateOwner>().unwrap();
    let model = builder.finish().unwrap();
    assert!(model["decisions"].as_array().unwrap().is_empty());
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
    outcomes: &[],
    implementation: DecisionImplementationDescriptor::Rust,
}];

struct EmptyOutcomeGroup;

#[derive(DomainIdentity)]
struct EmptyOutcomeIdentity(u64);

#[derive(Entity)]
#[domain(id = "empty-root", label = "Empty root")]
struct EmptyOutcomeRoot {
    #[domain(identity)]
    id: EmptyOutcomeIdentity,
}

impl domain::EntityDefinition for EmptyOutcomeRoot {
    type Owner = EmptyOutcomeOwner;
    type Identity = EmptyOutcomeIdentity;
}

#[derive(Aggregate)]
#[domain(id = "empty-outcome-owner", label = "Empty outcome owner")]
struct EmptyOutcomeOwner;

impl domain::AggregateDefinition for EmptyOutcomeOwner {
    type Context = DuplicateContext;
    type Root = EmptyOutcomeRoot;
    type Event = domain::NoDomainEvents;
}

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
    },
    DecisionOutcomeDescriptor {
        local_id: "same",
        label: "Second",
    },
];
const DUPLICATE_OUTCOME_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DUPLICATE_OUTCOME_DECISION_ID,
    label: "Duplicate",
    outcomes: DUPLICATE_LOCAL_OUTCOMES,
    implementation: DecisionImplementationDescriptor::Rust,
}];

struct DuplicateOutcomeGroup;

#[derive(DomainIdentity)]
struct DuplicateOutcomeIdentity(u64);

#[derive(Entity)]
#[domain(id = "duplicate-root", label = "Duplicate root")]
struct DuplicateOutcomeRoot {
    #[domain(identity)]
    id: DuplicateOutcomeIdentity,
}

impl domain::EntityDefinition for DuplicateOutcomeRoot {
    type Owner = DuplicateOutcomeOwner;
    type Identity = DuplicateOutcomeIdentity;
}

#[derive(Aggregate)]
#[domain(id = "duplicate-outcome-owner", label = "Duplicate outcome owner")]
struct DuplicateOutcomeOwner;

impl domain::AggregateDefinition for DuplicateOutcomeOwner {
    type Context = DuplicateContext;
    type Root = DuplicateOutcomeRoot;
    type Event = domain::NoDomainEvents;
}

impl DecisionGroupType for DuplicateOutcomeGroup {
    type Owner = DuplicateOutcomeOwner;

    const DECISIONS: &'static [DecisionDescriptor] = DUPLICATE_OUTCOME_DECISIONS;
}

#[test]
fn unattached_empty_outcome_group_is_not_registered() {
    let mut builder = DomainModelBuilder::new();
    builder.add_aggregate_type::<EmptyOutcomeOwner>().unwrap();
    assert!(
        builder.finish().unwrap()["decisions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unattached_duplicate_outcome_group_is_not_registered() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<DuplicateOutcomeOwner>()
        .unwrap();
    assert!(
        builder.finish().unwrap()["decisions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
