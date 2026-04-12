use serde_json::{Value, json};

use crate::EntityId;

use super::aggregate;

pub(super) fn project(id: EntityId) -> Value {
    json!({
        "aggregate": aggregate::project(id.aggregate),
        "local": id.local,
    })
}
