use std::io::{self, Write};

use serde_json::json;

use crate::{
    ActionId, ActionOwnerId, AggregateId, BoundedContextId, DecisionId, DecisionOwnerId,
    DomainServiceId, EntityId, EntityLifecycleId, InvariantId, InvariantOwnerId, ValueObjectId,
    ValueObjectOwnerId,
};

use super::{DomainTestDescriptor, DomainTestSubject, emitter, projection};

const CONTEXT: BoundedContextId = BoundedContextId("sales");
const AGGREGATE: AggregateId = AggregateId {
    context: CONTEXT,
    local: "order",
};
const ENTITY: EntityId = EntityId {
    aggregate: AGGREGATE,
    local: "line-item",
};
const SERVICE: DomainServiceId = DomainServiceId {
    context: CONTEXT,
    local: "checkout",
};
const VALUE_OBJECT: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::Entity(ENTITY),
    local: "quantity",
};

#[test]
fn projects_subjects_with_model_id_shapes() {
    let cases = [
        (
            DomainTestSubject::Action(ActionId {
                owner: ActionOwnerId::Aggregate(AGGREGATE),
                local: "submit",
            }),
            json!({
                "kind": "action",
                "id": {
                    "owner": {
                        "kind": "aggregate",
                        "id": { "context": "sales", "local": "order" },
                    },
                    "local": "submit",
                },
            }),
        ),
        (
            DomainTestSubject::Decision(DecisionId {
                owner: DecisionOwnerId::DomainService(SERVICE),
                local: "can-checkout",
            }),
            json!({
                "kind": "decision",
                "id": {
                    "owner": {
                        "kind": "domainService",
                        "id": { "context": "sales", "local": "checkout" },
                    },
                    "local": "can-checkout",
                },
            }),
        ),
        (
            DomainTestSubject::Invariant(InvariantId {
                owner: InvariantOwnerId::ValueObject(VALUE_OBJECT),
                local: "positive",
            }),
            json!({
                "kind": "invariant",
                "id": {
                    "owner": {
                        "kind": "valueObject",
                        "id": {
                            "owner": {
                                "kind": "entity",
                                "id": {
                                    "aggregate": {
                                        "context": "sales",
                                        "local": "order",
                                    },
                                    "local": "line-item",
                                },
                            },
                            "local": "quantity",
                        },
                    },
                    "local": "positive",
                },
            }),
        ),
        (
            DomainTestSubject::Lifecycle(EntityLifecycleId {
                owner: ENTITY,
                local: "fulfillment",
            }),
            json!({
                "kind": "lifecycle",
                "id": {
                    "owner": {
                        "aggregate": { "context": "sales", "local": "order" },
                        "local": "line-item",
                    },
                    "local": "fulfillment",
                },
            }),
        ),
    ];

    for (subject, expected_subject) in cases {
        assert_eq!(
            projection::project(descriptor(subject)),
            json!({
                "schemaVersion": 1,
                "package": "sales-domain",
                "target": "order-tests",
                "test": "accepts-valid-order",
                "file": "tests/order.rs",
                "line": 21,
                "column": 9,
                "subject": expected_subject,
            })
        );
    }
}

#[test]
fn compact_projection_is_deterministic_and_single_line() {
    let descriptor = descriptor(DomainTestSubject::Lifecycle(EntityLifecycleId {
        owner: ENTITY,
        local: "fulfillment",
    }));
    let first = projection::compact(descriptor);
    let second = projection::compact(descriptor);

    assert_eq!(first, second);
    assert!(!first.contains('\n'));
    assert!(!first.contains('\t'));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&first).unwrap(),
        projection::project(descriptor)
    );
}

#[test]
fn writer_emits_one_frame_and_surfaces_io_errors() {
    let descriptor = descriptor(DomainTestSubject::Action(ActionId {
        owner: ActionOwnerId::Entity(ENTITY),
        local: "reserve",
    }));
    let mut output = Vec::new();

    emitter::write_metadata(&mut output, descriptor).unwrap();

    let expected = format!(
        "\nROSTFREI_DOMAIN_TEST_METADATA_V1\t{}\n",
        projection::compact(descriptor)
    );
    assert_eq!(output, expected.as_bytes());
    assert_eq!(output.split(|byte| *byte == b'\n').count(), 3);

    let error = emitter::write_metadata(&mut FailingWriter, descriptor).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}

fn descriptor(subject: DomainTestSubject) -> DomainTestDescriptor {
    DomainTestDescriptor {
        package: "sales-domain",
        target: "order-tests",
        test: "accepts-valid-order",
        file: "tests/order.rs",
        line: 21,
        column: 9,
        subject,
    }
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
