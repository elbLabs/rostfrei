#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
use domain::{
    Aggregate, BoundedContext, BoundedContextId, DecisionDescriptor, DecisionId,
    DecisionImplementationDescriptor, DecisionInputDescriptor, DecisionOutputDescriptor,
    DecisionOwnerId, DomainIdentity, DomainService, DomainServiceDescriptor, DomainServiceId,
    DomainServiceType, Entity, ValueObject, ValueObjectType, domain_decisions, domain_model,
};
use serde_json::{Value, json};

#[derive(BoundedContext)]
#[domain(id = "decision-projection", label = "Decision projection")]
struct ProjectionContext;

#[derive(DomainIdentity)]
#[domain(owner = ProjectionRoot)]
struct ProjectionIdentity(u64);

#[domain_decisions(aggregate)]
trait PrimaryAggregateDecisions {
    #[decision(id = "evaluate-first", label = "Evaluate first")]
    fn evaluate_first(input: ProjectionInput) -> ProjectionOutput;

    #[decision(id = "evaluate-second", label = "Evaluate second")]
    fn evaluate_second(input: ProjectionInput) -> ProjectionOutput;
}

#[domain_decisions(aggregate)]
trait SharedAggregateDecisions {
    #[decision(id = "shared", label = "Aggregate shared")]
    fn shared(input: ProjectionInput) -> ProjectionOutput;
}

#[derive(Aggregate)]
#[domain(
    id = "projection-aggregate",
    label = "Projection aggregate",
    context = ProjectionContext,
    root = ProjectionRoot,
    decisions = [PrimaryAggregateDecisions, SharedAggregateDecisions]
)]
struct ProjectionAggregate;

#[domain_decisions(entity)]
trait SharedEntityDecisions {
    #[decision(id = "shared", label = "Entity shared")]
    fn shared(input: ProjectionInput) -> ProjectionOutput;
}

#[derive(Entity)]
#[domain(
    id = "projection-root",
    label = "Projection root",
    owner = ProjectionAggregate,
    decisions = [SharedEntityDecisions]
)]
struct ProjectionRoot {
    #[domain(identity)]
    id: ProjectionIdentity,
}

#[domain_decisions(value_object)]
trait SharedValueObjectDecisions {
    #[decision(id = "shared", label = "Value object shared")]
    fn shared(input: ProjectionInput) -> ProjectionOutput;
}

#[derive(ValueObject)]
#[domain(
    id = "projection-input",
    label = "Projection input",
    owner = ProjectionAggregate,
    decisions = [SharedValueObjectDecisions]
)]
struct ProjectionInput(u64);

#[derive(ValueObject)]
#[domain(
    id = "projection-output",
    label = "Projection output",
    owner = ProjectionAggregate
)]
struct ProjectionOutput(bool);

#[domain_decisions(domain_service)]
trait SharedServiceDecisions {
    #[decision(id = "shared", label = "Domain service shared")]
    fn shared(input: ProjectionInput) -> ProjectionOutput;
}

#[derive(DomainService)]
#[domain(
    id = "projection-service",
    label = "Projection service",
    context = ProjectionContext,
    decisions = [SharedServiceDecisions]
)]
struct ProjectionService;

fn projection_output() -> ProjectionOutput {
    ProjectionOutput(true)
}

impl PrimaryAggregateDecisions for ProjectionAggregate {
    fn evaluate_first(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }

    fn evaluate_second(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }
}

impl SharedAggregateDecisions for ProjectionAggregate {
    fn shared(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }
}

impl SharedEntityDecisions for ProjectionRoot {
    fn shared(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }
}

impl SharedValueObjectDecisions for ProjectionInput {
    fn shared(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }
}

impl SharedServiceDecisions for ProjectionService {
    fn shared(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }
}

fn projected_model() -> Value {
    domain_model! {
        contexts: [ProjectionContext],
        aggregates: [ProjectionAggregate],
        entities: [ProjectionRoot],
        identities: [ProjectionIdentity],
        value_objects: [ProjectionInput, ProjectionOutput],
        services: [ProjectionService],
        commands: [],
        errors: [],
        query_groups: [],
    }
}

#[domain_decisions(domain_service)]
trait FirstDuplicateDecisions {
    #[decision(id = "duplicate", label = "First duplicate")]
    fn first(input: ProjectionInput) -> ProjectionOutput;
}

#[domain_decisions(domain_service)]
trait SecondDuplicateDecisions {
    #[decision(id = "duplicate", label = "Second duplicate")]
    fn second(input: ProjectionInput) -> ProjectionOutput;
}

#[derive(DomainService)]
#[domain(
    id = "duplicate-service",
    label = "Duplicate service",
    context = ProjectionContext,
    decisions = [FirstDuplicateDecisions, SecondDuplicateDecisions]
)]
struct DuplicateService;

impl FirstDuplicateDecisions for DuplicateService {
    fn first(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }
}

impl SecondDuplicateDecisions for DuplicateService {
    fn second(_input: ProjectionInput) -> ProjectionOutput {
        projection_output()
    }
}

const MISMATCHED_SERVICE_ID: DomainServiceId = DomainServiceId {
    context: BoundedContextId("decision-projection"),
    local: "mismatched-service",
};
const FOREIGN_SERVICE_ID: DomainServiceId = DomainServiceId {
    context: BoundedContextId("decision-projection"),
    local: "foreign-service",
};
const MISMATCHED_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DecisionId {
        owner: DecisionOwnerId::DomainService(FOREIGN_SERVICE_ID),
        local: "mismatched",
    },
    label: "Mismatched",
    input: DecisionInputDescriptor::ValueObject(ProjectionInput::DESCRIPTOR.id),
    output: DecisionOutputDescriptor::ValueObject(ProjectionOutput::DESCRIPTOR.id),
    implementation: DecisionImplementationDescriptor::Rust,
}];

struct MismatchedService;

impl DomainServiceType for MismatchedService {
    type Context = ProjectionContext;

    const DESCRIPTOR: DomainServiceDescriptor = DomainServiceDescriptor {
        id: MISMATCHED_SERVICE_ID,
        label: "Mismatched service",
    };
    const DECISION_CONTRACTS: &'static [&'static [DecisionDescriptor]] = &[MISMATCHED_DECISIONS];
}

#[test]
fn projects_an_attached_rust_decision_as_exact_json() {
    assert_eq!(
        projected_model()["decisions"][0],
        json!({
            "id": {
                "owner": {
                    "kind": "aggregate",
                    "id": {
                        "context": "decision-projection",
                        "local": "projection-aggregate",
                    },
                },
                "local": "evaluate-first",
            },
            "label": "Evaluate first",
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
            "output": {
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
            "implementation": { "kind": "rust" },
        })
    );
}

#[test]
fn preserves_owner_attachment_and_source_order_for_all_owner_kinds() {
    let model = projected_model();
    let order = model["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|decision| {
            (
                decision["id"]["owner"]["kind"].as_str().unwrap(),
                decision["id"]["local"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        order,
        [
            ("aggregate", "evaluate-first"),
            ("aggregate", "evaluate-second"),
            ("aggregate", "shared"),
            ("entity", "shared"),
            ("valueObject", "shared"),
            ("domainService", "shared"),
        ]
    );
}

#[test]
#[should_panic(expected = "duplicate DecisionId")]
fn rejects_duplicate_decision_ids_across_attached_traits() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [],
        services: [DuplicateService],
        commands: [],
        errors: [],
        query_groups: [],
    };
}

#[test]
#[should_panic(expected = "decision descriptor owner mismatch")]
fn rejects_a_trusted_manual_descriptor_with_a_different_owner() {
    let mut builder = DomainModelBuilder::new();
    builder.add_domain_service_type::<MismatchedService>();
}

#[test]
fn accepts_the_same_local_id_on_different_owners() {
    let model = projected_model();
    let owner_kinds = model["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|decision| decision["id"]["local"] == "shared")
        .map(|decision| decision["id"]["owner"]["kind"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        owner_kinds,
        ["aggregate", "entity", "valueObject", "domainService"]
    );
}
