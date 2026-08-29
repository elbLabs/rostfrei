use async_trait::async_trait;
use rostfrei_core::{AggregateId, ContentFingerprint, OperationId};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchReceipt {
    duplicate: bool,
}

impl DispatchReceipt {
    pub const fn new(duplicate: bool) -> Self {
        Self { duplicate }
    }

    pub const fn duplicate(self) -> bool {
        self.duplicate
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
