use serde_json::{Value, json};

use crate::{
    AggregateId, DecisionId, DecisionOutcomeId, DecisionOwnerId, DomainErrorId, DomainIdentityId,
    EntityId, QueryId, ValueObjectId,
};

pub(super) fn decision(id: DecisionId) -> Value {
    json!({ "owner": decision_owner(id.owner), "local": id.local })
}

pub(super) fn decision_outcome(id: DecisionOutcomeId) -> Value {
    json!({ "decision": decision(id.decision), "local": id.local })
}

pub(super) fn decision_owner(id: DecisionOwnerId) -> Value {
    match id {
        DecisionOwnerId::Aggregate(id) => json!({
            "kind": "aggregate",
            "id": aggregate(id),
        }),
        DecisionOwnerId::Entity(id) => json!({
            "kind": "entity",
            "id": entity(id),
        }),
    }
}

pub(super) fn aggregate(id: AggregateId) -> Value {
    json!({
        "context": id.context.0,
        "local": id.local,
    })
}

pub(super) fn domain_error(id: DomainErrorId) -> Value {
    Value::String(id.0.to_owned())
}

pub(super) fn domain_identity(id: DomainIdentityId) -> Value {
    json!({ "owner": entity(id.owner) })
}

pub(super) fn entity(id: EntityId) -> Value {
    json!({
        "aggregate": aggregate(id.aggregate),
        "local": id.local,
    })
}

pub(super) fn query(id: QueryId) -> Value {
    json!({ "aggregate": aggregate(id.aggregate), "local": id.local })
}

pub(super) fn value_object(id: ValueObjectId) -> Value {
    Value::String(id.0.to_owned())
}
