use serde_json::{Value, json};

use crate::DomainServiceId;

pub(super) fn project(id: DomainServiceId) -> Value {
    json!({
        "context": id.context.0,
        "local": id.local,
    })
}
