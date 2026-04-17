#![allow(dead_code, non_snake_case)]

use domain::__private::DomainModelBuilder;
use domain::{
    Aggregate, BoundedContext, BoundedContextId, DomainIdentity, Entity, InvariantDescriptor,
    InvariantId, InvariantOwnerId, InvariantOwnerType, InvariantViolation, ValueObject,
    ValueObjectDescriptor, ValueObjectId, ValueObjectOwnerId, ValueObjectShapeDescriptor,
    ValueObjectType, domain_invariants, domain_model,
};
use serde_json::{Value, json};

#[derive(BoundedContext)]
#[domain(id = "invariant-projection", label = "Invariant projection")]
struct ProjectionContext;

#[domain_invariants(aggregate)]
trait PrimaryAggregateInvariants {
    #[invariant(id = "first", label = "First aggregate invariant")]
    fn first(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;

    #[invariant(id = "second", label = "Second aggregate invariant")]
    fn second(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[domain_invariants(aggregate)]
trait SharedAggregateInvariants {
    #[invariant(id = "shared", label = "Aggregate shared")]
    fn shared(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(DomainIdentity)]
#[domain(owner = ProjectionRoot)]
struct ProjectionIdentity(u64);

#[derive(Entity)]
#[domain(id = "projection-root", label = "Projection root", owner = ProjectionAggregate)]
struct ProjectionRoot {
    #[domain(identity)]
    id: ProjectionIdentity,
}

#[derive(Aggregate)]
#[domain(
    id = "projection-aggregate",
    label = "Projection aggregate",
    context = ProjectionContext,
    root = ProjectionRoot,
    invariants = [PrimaryAggregateInvariants, SharedAggregateInvariants]
)]
struct ProjectionAggregate;

impl PrimaryAggregateInvariants for ProjectionAggregate {
    fn first(_candidate: &ProjectionRoot) -> Option<InvariantViolation> {
        None
    }

    fn second(_candidate: &ProjectionRoot) -> Option<InvariantViolation> {
        None
    }
}

impl SharedAggregateInvariants for ProjectionAggregate {
    fn shared(_candidate: &ProjectionRoot) -> Option<InvariantViolation> {
        None
    }
}

#[domain_invariants(entity)]
trait SharedEntityInvariants {
    #[invariant(id = "shared", label = "Entity shared")]
    fn shared(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(DomainIdentity)]
#[domain(owner = ProjectionEntity)]
struct ProjectionEntityIdentity(u64);

#[derive(Entity)]
#[domain(
    id = "projection-entity",
    label = "Projection entity",
    owner = ProjectionAggregate,
    invariants = [SharedEntityInvariants]
)]
struct ProjectionEntity {
    #[domain(identity)]
    id: ProjectionEntityIdentity,
}

impl SharedEntityInvariants for ProjectionEntity {
    fn shared(_candidate: &Self) -> Option<InvariantViolation> {
        None
    }
}

#[domain_invariants(value_object)]
trait SharedValueObjectInvariants {
    #[invariant(id = "shared", label = "Value object shared")]
    fn shared(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(
    id = "projection-value",
    label = "Projection value",
    owner = ProjectionAggregate,
    invariants = [SharedValueObjectInvariants]
)]
struct ProjectionValue(u64);

impl SharedValueObjectInvariants for ProjectionValue {
    fn shared(_candidate: &Self) -> Option<InvariantViolation> {
        None
    }
}

#[domain_invariants(value_object)]
trait OmittedValueObjectInvariants {
    #[invariant(id = "omitted", label = "Omitted")]
    fn omitted(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(
    id = "omitted-value",
    label = "Omitted value",
    owner = ProjectionContext,
    invariants = [OmittedValueObjectInvariants]
)]
struct OmittedValue(u64);

impl OmittedValueObjectInvariants for OmittedValue {
    fn omitted(_candidate: &Self) -> Option<InvariantViolation> {
        None
    }
}

#[domain_invariants(value_object)]
trait UnattachedValueObjectInvariants {
    #[invariant(id = "unattached", label = "Unattached")]
    fn unattached(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(
    id = "unattached-value",
    label = "Unattached value",
    owner = ProjectionContext
)]
struct UnattachedValue(u64);

impl UnattachedValueObjectInvariants for UnattachedValue {
    fn unattached(_candidate: &Self) -> Option<InvariantViolation> {
        None
    }
}

fn projected_model() -> Value {
    domain_model! {
        contexts: [ProjectionContext],
        aggregates: [ProjectionAggregate],
        entities: [ProjectionRoot, ProjectionEntity],
        identities: [ProjectionIdentity, ProjectionEntityIdentity],
        value_objects: [ProjectionValue, UnattachedValue],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    }
}

#[domain_invariants(value_object)]
trait FirstDuplicateInvariants {
    #[invariant(id = "duplicate", label = "First duplicate")]
    fn first(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[domain_invariants(value_object)]
trait SecondDuplicateInvariants {
    #[invariant(id = "duplicate", label = "Second duplicate")]
    fn second(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(
    id = "duplicate-value",
    label = "Duplicate value",
    owner = ProjectionContext,
    invariants = [FirstDuplicateInvariants, SecondDuplicateInvariants]
)]
struct DuplicateValue(u64);

impl FirstDuplicateInvariants for DuplicateValue {
    fn first(_candidate: &Self) -> Option<InvariantViolation> {
        None
    }
}

impl SecondDuplicateInvariants for DuplicateValue {
    fn second(_candidate: &Self) -> Option<InvariantViolation> {
        None
    }
}

const MISMATCHED_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(BoundedContextId("invariant-projection")),
    local: "mismatched-value",
};
const FOREIGN_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(BoundedContextId("invariant-projection")),
    local: "foreign-value",
};
const MISMATCHED_INVARIANTS: &[InvariantDescriptor] = &[InvariantDescriptor {
    id: InvariantId {
        owner: InvariantOwnerId::ValueObject(FOREIGN_VALUE_ID),
        local: "mismatched",
    },
    label: "Mismatched",
}];

struct MismatchedValue;

impl ValueObjectType for MismatchedValue {
    type Owner = ProjectionContext;

    const LOCAL_ID: &'static str = "mismatched-value";
    const DESCRIPTOR: ValueObjectDescriptor = ValueObjectDescriptor {
        id: MISMATCHED_VALUE_ID,
        label: "Mismatched value",
        shape: ValueObjectShapeDescriptor::Struct { fields: &[] },
    };
    const INVARIANT_CONTRACTS: &'static [&'static [InvariantDescriptor]] = &[MISMATCHED_INVARIANTS];
}

#[test]
fn automatically_projects_flattened_invariants_in_owner_attachment_and_method_order() {
    assert_eq!(
        projected_model()["invariants"],
        json!([
            {
                "id": {
                    "owner": {
                        "kind": "aggregate",
                        "id": {
                            "context": "invariant-projection",
                            "local": "projection-aggregate"
                        }
                    },
                    "local": "first"
                },
                "label": "First aggregate invariant"
            },
            {
                "id": {
                    "owner": {
                        "kind": "aggregate",
                        "id": {
                            "context": "invariant-projection",
                            "local": "projection-aggregate"
                        }
                    },
                    "local": "second"
                },
                "label": "Second aggregate invariant"
            },
            {
                "id": {
                    "owner": {
                        "kind": "aggregate",
                        "id": {
                            "context": "invariant-projection",
                            "local": "projection-aggregate"
                        }
                    },
                    "local": "shared"
                },
                "label": "Aggregate shared"
            },
            {
                "id": {
                    "owner": {
                        "kind": "entity",
                        "id": {
                            "aggregate": {
                                "context": "invariant-projection",
                                "local": "projection-aggregate"
                            },
                            "local": "projection-entity"
                        }
                    },
                    "local": "shared"
                },
                "label": "Entity shared"
            },
            {
                "id": {
                    "owner": {
                        "kind": "valueObject",
                        "id": {
                            "owner": {
                                "kind": "aggregate",
                                "id": {
                                    "context": "invariant-projection",
                                    "local": "projection-aggregate"
                                }
                            },
                            "local": "projection-value"
                        }
                    },
                    "local": "shared"
                },
                "label": "Value object shared"
            }
        ])
    );
}

#[test]
fn does_not_project_omitted_or_unattached_owners() {
    let model = projected_model();
    let local_ids = model["invariants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|invariant| invariant["id"]["local"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert!(!local_ids.contains(&"omitted"));
    assert!(!local_ids.contains(&"unattached"));
}

#[test]
fn accepts_the_same_local_id_on_different_owners() {
    let model = projected_model();
    let owner_kinds = model["invariants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|invariant| invariant["id"]["local"] == "shared")
        .map(|invariant| invariant["id"]["owner"]["kind"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(owner_kinds, ["aggregate", "entity", "valueObject"]);
}

#[test]
#[should_panic(expected = "duplicate InvariantId")]
fn rejects_duplicate_ids_across_attached_traits() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [DuplicateValue],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    };
}

#[test]
#[should_panic(expected = "invariant descriptor owner mismatch")]
fn rejects_a_trusted_manual_descriptor_with_a_different_owner() {
    let mut builder = DomainModelBuilder::new();
    builder.add_value_object_type::<MismatchedValue>();
}
