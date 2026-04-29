#![allow(
    clippy::expect_used,
    reason = "test assertions require expected outcomes"
)]

use domain::__private::DomainModelBuilder;
use domain::{
    AggregateId, BoundedContextId, DomainEventDescriptor, DomainEventId, DomainIdentityId,
    EntityId, FieldDescriptor, FieldKind, FieldValue,
};

const CONTEXT: BoundedContextId = BoundedContextId("field-validation");
const AGGREGATE: AggregateId = AggregateId {
    context: CONTEXT,
    local: "owner",
};
const ENTITY: EntityId = EntityId {
    aggregate: AGGREGATE,
    local: "root",
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
fn rejects_missing_identity_reference() {
    rejects(
        FieldKind::DomainIdentity(DomainIdentityId { owner: ENTITY }),
        "references missing DomainIdentityId",
    );
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
