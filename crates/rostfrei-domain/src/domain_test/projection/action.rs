use serde_json::Value;

use crate::ActionId;

pub(super) fn project(id: ActionId) -> Value {
    Value::String(id.0.to_owned())
}
