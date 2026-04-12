use serde_json::{Value, json};

use crate::EntityLifecycleId;

use super::entity;

pub(super) fn project(id: EntityLifecycleId) -> Value {
    json!({
        "owner": entity::project(id.owner),
        "local": id.local,
    })
}
