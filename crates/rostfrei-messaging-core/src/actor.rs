use serde::{Deserialize, Serialize};

use crate::{ContractError, ContractErrorKind};

pub const MAX_ACTOR_NAMESPACE_BYTES: usize = 128;
pub const MAX_ACTOR_ID_BYTES: usize = 256;

/// Claimed original attribution, not an authenticated principal or permission grant.
/// Components are opaque, case-sensitive UTF-8 values; no provider syntax is inferred.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActorWire")]
pub struct ActorRef {
    namespace: String,
    id: String,
}

impl ActorRef {
    pub fn new(namespace: impl Into<String>, id: impl Into<String>) -> Result<Self, ContractError> {
        let namespace = namespace.into();
        let id = id.into();
        validate(&namespace, "actor namespace", MAX_ACTOR_NAMESPACE_BYTES)?;
        validate(&id, "actor id", MAX_ACTOR_ID_BYTES)?;
        Ok(Self { namespace, id })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorWire {
    namespace: String,
    id: String,
}

impl TryFrom<ActorWire> for ActorRef {
    type Error = ContractError;

    fn try_from(value: ActorWire) -> Result<Self, Self::Error> {
        Self::new(value.namespace, value.id)
    }
}

fn validate(value: &str, field: &'static str, maximum: usize) -> Result<(), ContractError> {
    if value.is_empty() {
        return Err(ContractError::new(ContractErrorKind::Empty, field));
    }
    if value.len() > maximum {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            field,
            value.len(),
            maximum,
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            field,
        ));
    }
    if value.trim() != value {
        return Err(ContractError::new(ContractErrorKind::InvalidFormat, field));
    }
    Ok(())
}
