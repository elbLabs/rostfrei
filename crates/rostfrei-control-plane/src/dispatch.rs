use std::sync::Arc;

use async_trait::async_trait;
use rostfrei_core::{AggregateId, ContentFingerprint, OperationId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct DispatchInvocation {
    operation_id: OperationId,
    operation_fingerprint: ContentFingerprint,
    aggregate_type: String,
    aggregate_id: AggregateId,
    command: String,
    schema_version: u32,
    payload: Value,
}

impl DispatchInvocation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        operation_fingerprint: ContentFingerprint,
        aggregate_type: impl Into<String>,
        aggregate_id: AggregateId,
        command: impl Into<String>,
        schema_version: u32,
        payload: Value,
    ) -> Self {
        Self {
            operation_id,
            operation_fingerprint,
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

    pub const fn operation_fingerprint(&self) -> ContentFingerprint {
        self.operation_fingerprint
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
pub struct DispatchPublication {
    command_message_id: String,
    duplicate: bool,
}

impl DispatchPublication {
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
pub trait DispatchObserver: Send + Sync {
    /// Records that command publication received its broker acknowledgement.
    ///
    /// Adapters must await this callback before returning from `dispatch`. The control plane
    /// retains a matching guard, but an adapter that detaches the callback can otherwise race a
    /// terminal result and lose publication evidence from the operation trace.
    async fn command_published(&self, publication: DispatchPublication);
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchRejection {
    pub classification: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl DispatchRejection {
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
pub enum DispatchOutcome {
    Accepted,
    Rejected(DispatchRejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchReceipt {
    command_message_id: String,
    response_message_id: String,
    duplicate: bool,
    outcome: DispatchOutcome,
}

impl DispatchReceipt {
    pub fn accepted(
        command_message_id: impl Into<String>,
        response_message_id: impl Into<String>,
        duplicate: bool,
    ) -> Self {
        Self {
            command_message_id: command_message_id.into(),
            response_message_id: response_message_id.into(),
            duplicate,
            outcome: DispatchOutcome::Accepted,
        }
    }

    pub fn rejected(
        command_message_id: impl Into<String>,
        response_message_id: impl Into<String>,
        duplicate: bool,
        rejection: DispatchRejection,
    ) -> Self {
        Self {
            command_message_id: command_message_id.into(),
            response_message_id: response_message_id.into(),
            duplicate,
            outcome: DispatchOutcome::Rejected(rejection),
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

    pub const fn outcome(&self) -> &DispatchOutcome {
        &self.outcome
    }

    pub fn into_parts(self) -> (String, String, bool, DispatchOutcome) {
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
pub enum DispatchErrorKind {
    #[error("dispatch request is invalid")]
    InvalidRequest,
    #[error("dispatch was rejected")]
    Rejected,
    #[error("dispatch confirmation timed out")]
    Timeout,
    #[error("dispatcher is unavailable")]
    Unavailable,
    #[error("dispatcher configuration is invalid")]
    InvalidConfiguration,
    #[error("dispatch response is invalid")]
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct DispatchError {
    kind: DispatchErrorKind,
    message: String,
}

impl DispatchError {
    pub fn new(kind: DispatchErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> DispatchErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[async_trait]
pub trait DispatchAdapter: Send + Sync {
    fn maximum_payload_len(&self) -> usize {
        usize::MAX
    }

    async fn dispatch(
        &self,
        invocation: DispatchInvocation,
        observer: Arc<dyn DispatchObserver>,
    ) -> Result<DispatchReceipt, DispatchError>;
}

pub fn dispatch_fingerprint(
    aggregate_type: &str,
    aggregate_id: &str,
    command: &str,
    schema_version: u32,
    payload: &Value,
) -> ContentFingerprint {
    let payload = payload.to_string();
    fingerprint(
        aggregate_type,
        aggregate_id,
        command,
        schema_version,
        payload.as_bytes(),
    )
}

fn fingerprint(
    aggregate_type: &str,
    aggregate_id: &str,
    command: &str,
    schema_version: u32,
    payload: &[u8],
) -> ContentFingerprint {
    let mut framed = Vec::new();
    let schema_version = schema_version.to_be_bytes();
    for value in [
        b"rostfrei:dispatch-request:v1".as_slice(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
        command.as_bytes(),
        schema_version.as_slice(),
        payload,
    ] {
        let length = u64::try_from(value.len()).unwrap_or(u64::MAX);
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(value);
    }
    ContentFingerprint::digest(framed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_fingerprint_is_stable_and_mode_scoped() {
        let fingerprint = dispatch_fingerprint(
            "bike-rental/rental-fleet",
            "city-fleet",
            "rent-bicycle",
            1,
            &serde_json::json!({ "bicycle_id": "bike-42" }),
        );

        assert_eq!(
            fingerprint,
            dispatch_fingerprint(
                "bike-rental/rental-fleet",
                "city-fleet",
                "rent-bicycle",
                1,
                &serde_json::json!({ "bicycle_id": "bike-42" }),
            )
        );
        assert_ne!(fingerprint, ContentFingerprint::digest("simulation"));
    }
}
