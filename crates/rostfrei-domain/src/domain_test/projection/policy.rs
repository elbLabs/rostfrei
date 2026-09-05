use serde_json::Value;

use crate::PolicyId;

pub(super) fn project(id: PolicyId) -> Value {
    Value::String(id.0.to_owned())
}
