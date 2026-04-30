use serde_json::{Value, json};

use crate::{AggregateId, DomainErrorId, DomainIdentityId, EntityId, ValueObjectId};

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

pub(super) fn value_object(id: ValueObjectId) -> Value {
    Value::String(id.0.to_owned())
}
