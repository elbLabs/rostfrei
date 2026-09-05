mod action;
mod invariant;
mod lifecycle;
mod policy;

use serde_json::{Value, json};

use super::{DomainTestDescriptor, DomainTestSubject};

pub(super) fn compact(descriptor: DomainTestDescriptor) -> String {
    project(descriptor).to_string()
}

pub(super) fn project(descriptor: DomainTestDescriptor) -> Value {
    json!({
        "schemaVersion": 2,
        "package": descriptor.package,
        "target": descriptor.target,
        "test": descriptor.test,
        "file": descriptor.file,
        "line": descriptor.line,
        "column": descriptor.column,
        "subject": subject(descriptor.subject),
    })
}

fn subject(subject: DomainTestSubject) -> Value {
    match subject {
        DomainTestSubject::Action(id) => json!({ "kind": "action", "id": action::project(id) }),
        DomainTestSubject::Policy(id) => {
            json!({ "kind": "policy", "id": policy::project(id) })
        }
        DomainTestSubject::Invariant(id) => {
            json!({ "kind": "invariant", "id": invariant::project(id) })
        }
        DomainTestSubject::Lifecycle(id) => {
            json!({ "kind": "lifecycle", "id": lifecycle::project(id) })
        }
    }
}
