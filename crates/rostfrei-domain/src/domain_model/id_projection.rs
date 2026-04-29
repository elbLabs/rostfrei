use serde_json::{Value, json};

use crate::{
    ActionId, ActionOwnerId, AggregateId, CommandId, CommandOwnerId, DecisionId, DecisionOutcomeId,
    DecisionOwnerId, DomainErrorId, DomainErrorOwnerId, DomainEventId, DomainIdentityId, EntityId,
    QueryId, ValueObjectId,
};

pub(super) fn action(id: ActionId) -> Value {
    json!({ "owner": action_owner(id.owner), "local": id.local })
}

pub(super) fn action_owner(id: ActionOwnerId) -> Value {
    match id {
        ActionOwnerId::Aggregate(id) => json!({
            "kind": "aggregate",
            "id": aggregate(id),
        }),
        ActionOwnerId::DomainService(id) => json!({
            "kind": "domainService",
            "id": {
                "context": id.context.0,
                "local": id.local,
            },
        }),
        ActionOwnerId::Entity(id) => json!({
            "kind": "entity",
            "id": entity(id),
        }),
    }
}

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

pub(super) fn command(id: CommandId) -> Value {
    json!({ "owner": command_owner(id.owner), "local": id.local })
}

pub(super) fn domain_error(id: DomainErrorId) -> Value {
    json!({ "owner": domain_error_owner(id.owner), "local": id.local })
}

pub(super) fn domain_error_owner(id: DomainErrorOwnerId) -> Value {
    match id {
        DomainErrorOwnerId::DomainService(id) => json!({
            "kind": "domainService",
            "id": {
                "context": id.context.0,
                "local": id.local,
            },
        }),
        DomainErrorOwnerId::Aggregate(id) => json!({
            "kind": "aggregate",
            "id": aggregate(id),
        }),
        DomainErrorOwnerId::Entity(id) => json!({
            "kind": "entity",
            "id": entity(id),
        }),
        DomainErrorOwnerId::ValueObject(id) => json!({
            "kind": "valueObject",
            "id": value_object(id),
        }),
    }
}

pub(super) fn domain_event(id: DomainEventId) -> Value {
    json!({ "aggregate": aggregate(id.aggregate), "local": id.local })
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

fn command_owner(id: CommandOwnerId) -> Value {
    match id {
        CommandOwnerId::Aggregate(id) => json!({
            "kind": "aggregate",
            "id": aggregate(id),
        }),
        CommandOwnerId::DomainService(id) => json!({
            "kind": "domainService",
            "id": { "context": id.context.0, "local": id.local },
        }),
    }
}
