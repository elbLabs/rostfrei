use serde_json::Value;

use crate::EntityLifecycleId;

pub(super) fn project(id: EntityLifecycleId) -> Value {
    Value::String(id.0.to_owned())
}
