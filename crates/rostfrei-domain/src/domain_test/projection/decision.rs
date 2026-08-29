use serde_json::{Value, json};

use crate::{DecisionId, DecisionOwnerId};

use super::{aggregate, entity};

pub(super) fn project(id: DecisionId) -> Value {
    json!({
        "owner": owner(id.owner),
        "local": id.local,
    })
}

fn owner(id: DecisionOwnerId) -> Value {
    match id {
        DecisionOwnerId::Aggregate(id) => {
            json!({ "kind": "aggregate", "id": aggregate::project(id) })
        }
        DecisionOwnerId::Entity(id) => {
            json!({ "kind": "entity", "id": entity::project(id) })
        }
    }
}
