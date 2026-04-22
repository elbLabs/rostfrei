use std::sync::Arc;

use async_trait::async_trait;
use rostfrei_core::{AggregateId, ContentFingerprint, OperationId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct CommandInvocation {
    operation_id: OperationId,
    correlation_id: String,
    execution_fingerprint: ContentFingerprint,
    aggregate_type: String,
    aggregate_id: AggregateId,
    command: String,
    schema_version: u32,
    payload: Value,
}

impl CommandInvocation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        correlation_id: impl Into<String>,
        execution_fingerprint: ContentFingerprint,
        aggregate_type: impl Into<String>,
        aggregate_id: AggregateId,
        command: impl Into<String>,
        schema_version: u32,
        payload: Value,
    ) -> Self {
        Self {
            operation_id,
            correlation_id: correlation_id.into(),
            execution_fingerprint,
            aggregate_type: aggregate_type.into(),
            aggregate_id,
            command: command.into(),
            schema_version,
            payload,
        }
    }

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub const fn execution_fingerprint(&self) -> ContentFingerprint {
        self.execution_fingerprint
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub const fn aggregate_id(&self) -> &AggregateId {
        &self.aggregate_id
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn payload(&self) -> &Value {
        &self.payload
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPublication {
    command_message_id: String,
    duplicate: bool,
}

impl CommandPublication {
    pub fn new(command_message_id: impl Into<String>, duplicate: bool) -> Self {
        Self {
            command_message_id: command_message_id.into(),
            duplicate,
        }
    }

    pub fn command_message_id(&self) -> &str {
        &self.command_message_id
    }

    pub const fn duplicate(&self) -> bool {
        self.duplicate
    }
}

#[async_trait]
pub trait CommandTransportObserver: Send + Sync {
    /// Records that command publication received its broker acknowledgement.
    ///
    /// Transports must await this callback before returning a receipt. This preserves
    /// publication evidence before the operation records its durable response.
    async fn command_published(&self, publication: CommandPublication);
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRejection {
    pub classification: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl CommandRejection {
    pub fn new(
        classification: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self {
            classification: classification.into(),
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    pub fn into_value(self) -> Value {
        let mut value = serde_json::Map::from_iter([
            (
                "classification".to_owned(),
                Value::String(self.classification),
            ),
            ("code".to_owned(), Value::String(self.code)),
            ("message".to_owned(), Value::String(self.message)),
        ]);
        if let Some(details) = self.details {
            value.insert("details".to_owned(), details);
        }
        Value::Object(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    Accepted,
    Rejected(CommandRejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandReceipt {
    command_message_id: String,
    response_message_id: String,
    duplicate: bool,
    outcome: CommandOutcome,
}

impl CommandReceipt {
    pub fn accepted(
        command_message_id: impl Into<String>,
        response_message_id: impl Into<String>,
        duplicate: bool,
    ) -> Self {
        Self {
            command_message_id: command_message_id.into(),
            response_message_id: response_message_id.into(),
            duplicate,
            outcome: CommandOutcome::Accepted,
        }
    }

    pub fn rejected(
        command_message_id: impl Into<String>,
        response_message_id: impl Into<String>,
        duplicate: bool,
        rejection: CommandRejection,
    ) -> Self {
        Self {
            command_message_id: command_message_id.into(),
            response_message_id: response_message_id.into(),
            duplicate,
            outcome: CommandOutcome::Rejected(rejection),
        }
    }

    pub fn command_message_id(&self) -> &str {
        &self.command_message_id
    }

    pub fn response_message_id(&self) -> &str {
        &self.response_message_id
    }

    pub const fn duplicate(&self) -> bool {
        self.duplicate
    }

    pub const fn outcome(&self) -> &CommandOutcome {
        &self.outcome
    }

    pub fn into_parts(self) -> (String, String, bool, CommandOutcome) {
        (
            self.command_message_id,
            self.response_message_id,
            self.duplicate,
            self.outcome,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum CommandTransportErrorKind {
    #[error("command transport request is invalid")]
    InvalidRequest,
    #[error("command transport rejected the request")]
    Rejected,
    #[error("command response timed out")]
    Timeout,
    #[error("command transport is unavailable")]
    Unavailable,
    #[error("command transport configuration is invalid")]
    InvalidConfiguration,
    #[error("command transport response is invalid")]
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct CommandTransportError {
    kind: CommandTransportErrorKind,
    message: String,
}

impl CommandTransportError {
    pub fn new(kind: CommandTransportErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> CommandTransportErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[async_trait]
pub trait CommandTransport: Send + Sync {
    fn maximum_payload_len(&self) -> usize {
        usize::MAX
    }

    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError>;
}

pub fn command_execution_fingerprint(
    aggregate_type: &str,
    aggregate_id: &str,
    command: &str,
    schema_version: u32,
    payload: &Value,
) -> ContentFingerprint {
    let payload = canonical_json_payload(payload);
    let schema_version = schema_version.to_be_bytes();
    framed_fingerprint(&[
        b"rostfrei:command-execution:v1".as_slice(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
        command.as_bytes(),
        schema_version.as_slice(),
        &payload,
    ])
}

pub(crate) fn canonical_json_payload(value: &Value) -> Vec<u8> {
    let mut serialized = Vec::new();
    write_canonical_json(value, &mut serialized);
    serialized
}

fn write_canonical_json(value: &Value, serialized: &mut Vec<u8>) {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serialized.extend_from_slice(value.to_string().as_bytes());
        }
        Value::Array(values) => {
            serialized.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    serialized.push(b',');
                }
                write_canonical_json(value, serialized);
            }
            serialized.push(b']');
        }
        Value::Object(values) => {
            serialized.push(b'{');
            let mut fields = values.iter().collect::<Vec<_>>();
            fields.sort_unstable_by_key(|(name, _)| *name);
            for (index, (name, value)) in fields.into_iter().enumerate() {
                if index != 0 {
                    serialized.push(b',');
                }
                serialized.extend_from_slice(Value::String(name.clone()).to_string().as_bytes());
                serialized.push(b':');
                write_canonical_json(value, serialized);
            }
            serialized.push(b'}');
        }
    }
}

fn framed_fingerprint(values: &[&[u8]]) -> ContentFingerprint {
    let mut framed = Vec::new();
    for value in values {
        let length = usize_to_u64(value.len());
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(value);
    }
    ContentFingerprint::digest(framed)
}

const fn usize_to_u64(value: usize) -> u64 {
    #[cfg(target_pointer_width = "16")]
    {
        u64::from(u16::from_ne_bytes(value.to_ne_bytes()))
    }
    #[cfg(target_pointer_width = "32")]
    {
        u64::from(u32::from_ne_bytes(value.to_ne_bytes()))
    }
    #[cfg(target_pointer_width = "64")]
    {
        u64::from_ne_bytes(value.to_ne_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_fingerprint_canonicalizes_object_keys() {
        let left: Value = serde_json::from_str(r#"{"outer":{"b":2,"a":1},"z":false}"#).unwrap();
        let right: Value = serde_json::from_str(r#"{"z":false,"outer":{"a":1,"b":2}}"#).unwrap();

        assert_eq!(
            command_execution_fingerprint("context/aggregate", "one", "command", 1, &left),
            command_execution_fingerprint("context/aggregate", "one", "command", 1, &right)
        );
        assert_ne!(
            command_execution_fingerprint("context/aggregate", "one", "command", 1, &left),
            command_execution_fingerprint("context/aggregate", "two", "command", 1, &left)
        );
    }
}
