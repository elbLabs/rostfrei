use std::io::{self, Write};

use serde_json::json;

use crate::{ActionId, EntityLifecycleId, InvariantId, PolicyId};

use super::{DomainTestDescriptor, DomainTestSubject, emitter, projection};

#[test]
fn projects_global_subject_ids_consistently() {
    let cases = [
        (
            DomainTestSubject::Action(ActionId("submit")),
            json!({
                "kind": "action",
                "id": "submit",
            }),
        ),
        (
            DomainTestSubject::Policy(PolicyId("can-submit")),
            json!({
                "kind": "policy",
                "id": "can-submit",
            }),
        ),
        (
            DomainTestSubject::Invariant(InvariantId("positive")),
            json!({
                "kind": "invariant",
                "id": "positive",
            }),
        ),
        (
            DomainTestSubject::Lifecycle(EntityLifecycleId("fulfillment")),
            json!({
                "kind": "lifecycle",
                "id": "fulfillment",
            }),
        ),
    ];

    for (subject, expected_subject) in cases {
        assert_eq!(
            projection::project(descriptor(subject)),
            json!({
                "schemaVersion": 2,
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
    let descriptor = descriptor(DomainTestSubject::Lifecycle(EntityLifecycleId(
        "fulfillment",
    )));
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
    let descriptor = descriptor(DomainTestSubject::Action(ActionId("reserve")));
    let mut output = Vec::new();

    emitter::write_metadata(&mut output, descriptor).unwrap();

    let expected = format!(
        "\nROSTFREI_DOMAIN_TEST_METADATA_V2\t{}\n",
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
