use serde_json::{Value, json};

use crate::{ActionId, ActionOwnerId};

use super::{aggregate, domain_service, entity, value_object};

pub(super) fn project(id: ActionId) -> Value {
    json!({
        "owner": owner(id.owner),
        "local": id.local,
    })
}

fn owner(id: ActionOwnerId) -> Value {
    match id {
        ActionOwnerId::Aggregate(id) => {
            json!({ "kind": "aggregate", "id": aggregate::project(id) })
        }
        ActionOwnerId::DomainService(id) => {
            json!({ "kind": "domainService", "id": domain_service::project(id) })
        }
        ActionOwnerId::Entity(id) => {
            json!({ "kind": "entity", "id": entity::project(id) })
        }
        ActionOwnerId::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object::project(id) })
        }
    }
}
