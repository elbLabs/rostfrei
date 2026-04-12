use serde_json::{Value, json};

use crate::{
    ActionId, ActionOwnerId, AggregateId, DecisionOwnerId, DomainCommandId, DomainCommandOwnerId,
    DomainErrorId, DomainErrorOwnerId, DomainEventId, DomainIdentityId, EntityId, InvariantOwnerId,
    QueryId, ValueObjectId, ValueObjectOwnerId,
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
        ActionOwnerId::ValueObject(id) => json!({
            "kind": "valueObject",
            "id": value_object(id),
        }),
    }
}

pub(super) fn decision_owner(id: DecisionOwnerId) -> Value {
    match id {
        DecisionOwnerId::Aggregate(id) => json!({
            "kind": "aggregate",
            "id": aggregate(id),
        }),
        DecisionOwnerId::DomainService(id) => json!({
            "kind": "domainService",
            "id": {
                "context": id.context.0,
                "local": id.local,
            },
        }),
        DecisionOwnerId::Entity(id) => json!({
            "kind": "entity",
            "id": entity(id),
        }),
        DecisionOwnerId::ValueObject(id) => json!({
            "kind": "valueObject",
            "id": value_object(id),
        }),
    }
}

pub(super) fn aggregate(id: AggregateId) -> Value {
    json!({
        "context": id.context.0,
        "local": id.local,
    })
}

pub(super) fn domain_command(id: DomainCommandId) -> Value {
    json!({ "owner": domain_command_owner(id.owner), "local": id.local })
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

pub(super) fn invariant_owner(id: InvariantOwnerId) -> Value {
    match id {
        InvariantOwnerId::Aggregate(id) => json!({ "kind": "aggregate", "id": aggregate(id) }),
        InvariantOwnerId::Entity(id) => json!({ "kind": "entity", "id": entity(id) }),
        InvariantOwnerId::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object(id) })
        }
    }
}

pub(super) fn query(id: QueryId) -> Value {
    json!({ "aggregate": aggregate(id.aggregate), "local": id.local })
}

pub(super) fn value_object(id: ValueObjectId) -> Value {
    json!({
        "owner": value_object_owner(id.owner),
        "local": id.local,
    })
}

fn domain_command_owner(id: DomainCommandOwnerId) -> Value {
    match id {
        DomainCommandOwnerId::Aggregate(id) => json!({
            "kind": "aggregate",
            "id": aggregate(id),
        }),
        DomainCommandOwnerId::DomainService(id) => json!({
            "kind": "domainService",
            "id": { "context": id.context.0, "local": id.local },
        }),
    }
}

fn value_object_owner(id: ValueObjectOwnerId) -> Value {
    match id {
        ValueObjectOwnerId::BoundedContext(id) => json!({
            "kind": "boundedContext",
            "id": id.0,
        }),
        ValueObjectOwnerId::Aggregate(id) => json!({
            "kind": "aggregate",
            "id": aggregate(id),
        }),
        ValueObjectOwnerId::Entity(id) => json!({
            "kind": "entity",
            "id": entity(id),
        }),
    }
}
