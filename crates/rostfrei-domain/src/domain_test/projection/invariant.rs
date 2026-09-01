use serde_json::Value;

use crate::InvariantId;

pub(super) fn project(id: InvariantId) -> Value {
    Value::String(id.0.to_owned())
}
