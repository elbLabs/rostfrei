use serde_json::{Value, json};

use crate::{ValueObjectId, ValueObjectOwnerId};

use super::{aggregate, entity};

pub(super) fn project(id: ValueObjectId) -> Value {
    json!({
        "owner": owner(id.owner),
        "local": id.local,
    })
}

fn owner(id: ValueObjectOwnerId) -> Value {
    match id {
        ValueObjectOwnerId::BoundedContext(id) => {
            json!({ "kind": "boundedContext", "id": id.0 })
        }
        ValueObjectOwnerId::Aggregate(id) => {
            json!({ "kind": "aggregate", "id": aggregate::project(id) })
        }
        ValueObjectOwnerId::Entity(id) => {
            json!({ "kind": "entity", "id": entity::project(id) })
        }
    }
}
