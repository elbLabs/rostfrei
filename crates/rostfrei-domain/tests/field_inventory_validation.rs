#![allow(
    clippy::expect_used,
    reason = "test assertions require expected outcomes"
)]

use domain::__private::DomainModelBuilder;
use domain::{
    AggregateId, BoundedContextId, DomainEventDescriptor, DomainEventId, EntityId, FieldDescriptor,
    FieldKind, FieldValue,
};
use serde_json::json;

const CONTEXT: BoundedContextId = BoundedContextId("field-validation");
const AGGREGATE: AggregateId = AggregateId {
    context: CONTEXT,
    local: "owner",
};
const MISSING_ENTITY: EntityId = EntityId {
    aggregate: AGGREGATE,
    local: "missing",
};
const MISSING_AGGREGATE: AggregateId = AggregateId {
    context: CONTEXT,
    local: "missing",
};

fn event(field: FieldKind) -> DomainEventDescriptor {
    DomainEventDescriptor {
        id: DomainEventId {
            aggregate: AGGREGATE,
            local: "inspected",
        },
        label: "Inspected",
        schema_version: 1,
        fields: Box::leak(Box::new([FieldDescriptor {
            name: "reference",
            value: FieldValue {
                kind: field,
                wrappers: &[],
            },
        }])),
    }
}

fn rejects(field: FieldKind, expected: &str) {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_domain_event(event(field))
        .expect("fixture event should register");
    let error = builder
        .finish()
        .expect_err("missing field reference should be rejected");
    assert!(error.to_string().contains(expected));
}

#[test]
fn rejects_missing_entity_reference() {
    rejects(
        FieldKind::Entity(MISSING_ENTITY),
        "references missing EntityId",
    );
}

#[test]
fn rejects_missing_aggregate_reference() {
    rejects(
        FieldKind::AggregateReference(MISSING_AGGREGATE),
        "references missing AggregateId",
    );
}

#[test]
fn handwritten_fields_without_type_evidence_keep_their_projection() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_domain_event(event(FieldKind::Opaque))
        .expect("fixture event should register");
    let model = builder.finish().expect("an opaque field is valid");
    assert_eq!(
        model["domainEvents"][0]["fields"],
        json!([{ "name": "reference", "value": { "kind": "opaque" } }])
    );
}
