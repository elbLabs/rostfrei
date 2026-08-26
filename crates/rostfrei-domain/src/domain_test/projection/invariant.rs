use serde_json::{Value, json};

use crate::{InvariantId, InvariantOwnerId};

use super::{aggregate, entity, value_object};

pub(super) fn project(id: InvariantId) -> Value {
    json!({
        "owner": owner(id.owner),
        "local": id.local,
    })
}

fn owner(id: InvariantOwnerId) -> Value {
    match id {
        InvariantOwnerId::Aggregate(id) => {
            json!({ "kind": "aggregate", "id": aggregate::project(id) })
        }
        InvariantOwnerId::Entity(id) => {
            json!({ "kind": "entity", "id": entity::project(id) })
        }
        InvariantOwnerId::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object::project(id) })
        }
    }
}
