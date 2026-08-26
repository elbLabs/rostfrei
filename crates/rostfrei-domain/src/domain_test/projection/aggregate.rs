use serde_json::{Value, json};

use crate::AggregateId;

pub(super) fn project(id: AggregateId) -> Value {
    json!({
        "context": id.context.0,
        "local": id.local,
    })
}
