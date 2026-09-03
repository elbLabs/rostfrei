use serde_json::Value;

use crate::DecisionId;

pub(super) fn project(id: DecisionId) -> Value {
    Value::String(id.0.to_owned())
}
