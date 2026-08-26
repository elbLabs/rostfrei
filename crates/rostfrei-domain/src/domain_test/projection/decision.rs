use serde_json::{Value, json};

use crate::{DecisionId, DecisionOwnerId};

use super::{aggregate, domain_service, entity, value_object};

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
        DecisionOwnerId::DomainService(id) => {
            json!({ "kind": "domainService", "id": domain_service::project(id) })
        }
        DecisionOwnerId::Entity(id) => {
            json!({ "kind": "entity", "id": entity::project(id) })
        }
        DecisionOwnerId::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object::project(id) })
        }
    }
}
