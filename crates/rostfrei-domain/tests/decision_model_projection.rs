#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, DomainModelError, Entity, ValueObject,
    domain_decisions, domain_model,
};
use serde_json::{Value, json};

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
    decisions
)]
struct ProjectionAggregate;

#[derive(Entity)]
#[domain(
    id = "projection-root",
    label = "Projection root",
    owner = ProjectionAggregate,
    decisions
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
    id = "projection-error",
    label = "Projection error",
    owner = ProjectionAggregate
)]
struct ProjectionError;

#[domain_decisions(aggregate)]
impl ProjectionAggregate {
    #[decision(id = "evaluate", label = "Evaluate")]
    const fn evaluate(
        input: ProjectionInput,
        threshold: u64,
    ) -> Result<ProjectionOutput, ProjectionError> {
        if input.0 >= threshold {
            Ok(ProjectionOutput(true))
        } else {
            Err(ProjectionError)
        }
    }

    #[decision(id = "shared", label = "Aggregate shared")]
    fn shared(input: ProjectionInput) -> Result<(), ProjectionError> {
        (input.0 > 0).then_some(()).ok_or(ProjectionError)
    }
}

#[domain_decisions(entity)]
impl ProjectionRoot {
    #[decision(id = "shared", label = "Entity shared")]
    const fn shared(input: ProjectionInput) -> Result<ProjectionOutput, ProjectionError> {
        if input.0 > 0 {
            Ok(ProjectionOutput(true))
        } else {
            Err(ProjectionError)
        }
    }
}

fn projected_model() -> Result<Value, DomainModelError> {
    domain_model! {
        contexts: [ProjectionContext],
        aggregates: [ProjectionAggregate],
        entities: [ProjectionRoot],
        identities: [ProjectionIdentity],
        value_objects: [ProjectionInput, ProjectionOutput, ProjectionError],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    }
}

#[test]
fn projects_result_signature_and_named_parameters_as_exact_json() {
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
            "error": {
                "kind": "valueObject",
                "id": {
                    "owner": {
                        "kind": "aggregate",
                        "id": {
                            "context": "decision-projection",
                            "local": "projection-aggregate",
                        },
                    },
                    "local": "projection-error",
                },
            },
            "implementation": { "kind": "rust" },
        })
    );
}

#[test]
fn preserves_owner_and_method_order_and_allows_local_ids_on_different_owners() {
    let model = projected_model().expect("decision model projection should succeed");
    let decisions = model["decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), 3);
    assert_eq!(decisions[0]["id"]["local"], "evaluate");
    assert_eq!(decisions[1]["id"]["local"], "shared");
    assert_eq!(decisions[1]["id"]["owner"]["kind"], "aggregate");
    assert_eq!(decisions[2]["id"]["local"], "shared");
    assert_eq!(decisions[2]["id"]["owner"]["kind"], "entity");
    assert_eq!(decisions[1]["output"], Value::Null);
}
