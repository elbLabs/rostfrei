use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use sha2::{Digest, Sha256};

use crate::{
    ApplicationErrorCode, CommandAddress, CommandResponseAddress, CommandResponseReadError,
    CommandResponseReadErrorKind, ContractError, ContractErrorKind, CorrelationId,
    MessageBuildError, MessageId, OperationId, SchemaVersion, envelope::validate_serialized_size,
};

pub const MAX_COMMAND_REJECTION_MESSAGE_BYTES: usize = 1024;
pub const MAX_COMMAND_RESPONSE_TIMEOUT: Duration = Duration::from_hours(24);
pub const COMMAND_RESPONSE_SCHEMA_VERSION: u32 = 1;
const RESPONSE_ADDRESS_HASH_DOMAIN: &[u8] = b"rostfrei.command-response-address.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CommandRejectionClassification {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Unavailable,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandRejection {
    classification: CommandRejectionClassification,
    code: ApplicationErrorCode,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl CommandRejection {
    pub fn new(
        classification: CommandRejectionClassification,
        code: ApplicationErrorCode,
        message: impl Into<String>,
        details: Option<serde_json::Value>,
    ) -> Result<Self, ContractError> {
        let message = message.into();
        validate_rejection_message(&message)?;
        Ok(Self {
            classification,
            code,
            message,
            details,
        })
    }

    pub const fn classification(&self) -> CommandRejectionClassification {
        self.classification
    }

    pub const fn code(&self) -> &ApplicationErrorCode {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn details(&self) -> Option<&serde_json::Value> {
        self.details.as_ref()
    }
}

#[derive(Deserialize)]
struct CommandRejectionWire {
    classification: CommandRejectionClassification,
    code: ApplicationErrorCode,
    message: String,
    details: Option<serde_json::Value>,
}

impl<'de> Deserialize<'de> for CommandRejection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CommandRejectionWire::deserialize(deserializer)?;
        Self::new(wire.classification, wire.code, wire.message, wire.details)
            .map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum CommandResponseOutcome {
    Accepted,
    Rejected(CommandRejection),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandResponse {
    message_id: MessageId,
    command_message_id: MessageId,
    command_address: CommandAddress,
    operation_id: OperationId,
    schema_version: SchemaVersion,
    correlation_id: CorrelationId,
    outcome: CommandResponseOutcome,
}

impl CommandResponse {
    pub fn accepted(
        message_id: MessageId,
        command_message_id: MessageId,
        command_address: CommandAddress,
        operation_id: OperationId,
        correlation_id: CorrelationId,
    ) -> Result<Self, MessageBuildError> {
        Self::new(
            message_id,
            command_message_id,
            command_address,
            operation_id,
            command_response_schema_version()?,
            correlation_id,
            CommandResponseOutcome::Accepted,
        )
    }

    pub fn rejected(
        message_id: MessageId,
        command_message_id: MessageId,
        command_address: CommandAddress,
        operation_id: OperationId,
        correlation_id: CorrelationId,
        rejection: CommandRejection,
    ) -> Result<Self, MessageBuildError> {
        Self::new(
            message_id,
            command_message_id,
            command_address,
            operation_id,
            command_response_schema_version()?,
            correlation_id,
            CommandResponseOutcome::Rejected(rejection),
        )
    }

    fn new(
        message_id: MessageId,
        command_message_id: MessageId,
        command_address: CommandAddress,
        operation_id: OperationId,
        schema_version: SchemaVersion,
        correlation_id: CorrelationId,
        outcome: CommandResponseOutcome,
    ) -> Result<Self, MessageBuildError> {
        if schema_version.get() != COMMAND_RESPONSE_SCHEMA_VERSION {
            return Err(MessageBuildError::serialization());
        }
        let response = Self {
            message_id,
            command_message_id,
            command_address,
            operation_id,
            schema_version,
            correlation_id,
            outcome,
        };
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), MessageBuildError> {
        validate_serialized_size(self)
    }

    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub const fn command_message_id(&self) -> &MessageId {
        &self.command_message_id
    }

    pub const fn command_address(&self) -> &CommandAddress {
        &self.command_address
    }

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn outcome(&self) -> &CommandResponseOutcome {
        &self.outcome
    }

    pub fn into_outcome(self) -> CommandResponseOutcome {
        self.outcome
    }
}

#[derive(Deserialize)]
struct CommandResponseWire {
    message_id: MessageId,
    command_message_id: MessageId,
    command_address: CommandAddress,
    operation_id: OperationId,
    schema_version: SchemaVersion,
    correlation_id: CorrelationId,
    outcome: CommandResponseOutcome,
}

impl<'de> Deserialize<'de> for CommandResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CommandResponseWire::deserialize(deserializer)?;
        Self::new(
            wire.message_id,
            wire.command_message_id,
            wire.command_address,
            wire.operation_id,
            wire.schema_version,
            wire.correlation_id,
            wire.outcome,
        )
        .map_err(D::Error::custom)
    }
}

fn command_response_schema_version() -> Result<SchemaVersion, MessageBuildError> {
    SchemaVersion::new(COMMAND_RESPONSE_SCHEMA_VERSION)
        .map_err(|_| MessageBuildError::serialization())
}

/// Derives an exact response address from length-prefixed, versioned SHA-256 input frames.
pub fn derive_command_response_address(
    command_address: &CommandAddress,
    operation_id: &OperationId,
    command_message_id: &MessageId,
) -> Result<CommandResponseAddress, ContractError> {
    let mut hash = Sha256::new();
    for value in [
        RESPONSE_ADDRESS_HASH_DOMAIN,
        command_address.as_str().as_bytes(),
        operation_id.as_str().as_bytes(),
        command_message_id.as_str().as_bytes(),
    ] {
        let length = u64::try_from(value.len()).map_err(|_| {
            ContractError::new(ContractErrorKind::OutOfRange, "command response hash frame")
        })?;
        hash.update(length.to_be_bytes());
        hash.update(value);
    }
    let name = format!("{:x}", hash.finalize());
    CommandResponseAddress::new_in_scope(
        command_address.application(),
        command_address.traffic_scope(),
        command_address.context(),
        &name,
    )
}

#[async_trait]
/// Reads a durably stored response without consuming it.
///
/// Repeated reads for the same identity must return the same response while the
/// transport's documented response-retention window remains open.
pub trait CommandResponseReader: Send + Sync {
    /// Looks up one exact retained response without waiting for its publication.
    async fn find_command_response(
        &self,
        address: &CommandResponseAddress,
        expected_operation_id: &OperationId,
        expected_command_message_id: &MessageId,
        timeout: Duration,
    ) -> Result<Option<CommandResponse>, CommandResponseReadError> {
        match self
            .read_command_response(
                address,
                expected_operation_id,
                expected_command_message_id,
                timeout,
            )
            .await
        {
            Ok(response) => Ok(Some(response)),
            Err(error) if error.kind() == CommandResponseReadErrorKind::Timeout => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn read_command_response(
        &self,
        address: &CommandResponseAddress,
        expected_operation_id: &OperationId,
        expected_command_message_id: &MessageId,
        timeout: Duration,
    ) -> Result<CommandResponse, CommandResponseReadError>;
}

fn validate_rejection_message(message: &str) -> Result<(), ContractError> {
    if message.is_empty() {
        return Err(ContractError::new(
            ContractErrorKind::Empty,
            "command rejection message",
        ));
    }
    if message.len() > MAX_COMMAND_REJECTION_MESSAGE_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            "command rejection message",
            message.len(),
            MAX_COMMAND_REJECTION_MESSAGE_BYTES,
        ));
    }
    if message.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            "command rejection message",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAX_ENVELOPE_BYTES, MessageBuildErrorKind};

    fn response(outcome: CommandResponseOutcome) -> CommandResponse {
        CommandResponse::new(
            MessageId::new("response-1").unwrap(),
            MessageId::new("command-1").unwrap(),
            CommandAddress::new("acme", "orders", "place-order").unwrap(),
            OperationId::new("operation-1").unwrap(),
            SchemaVersion::new(COMMAND_RESPONSE_SCHEMA_VERSION).unwrap(),
            CorrelationId::new("correlation-1").unwrap(),
            outcome,
        )
        .unwrap()
    }

    #[test]
    fn accepted_and_rejected_responses_preserve_durable_identity() {
        let accepted = response(CommandResponseOutcome::Accepted);
        assert_eq!(accepted.message_id().as_str(), "response-1");
        assert_eq!(accepted.command_message_id().as_str(), "command-1");
        assert_eq!(
            accepted.command_address().as_str(),
            "acme.command.orders.place-order"
        );
        assert_eq!(accepted.operation_id().as_str(), "operation-1");
        assert_eq!(
            accepted.schema_version().get(),
            COMMAND_RESPONSE_SCHEMA_VERSION
        );
        assert_eq!(accepted.correlation_id().as_str(), "correlation-1");
        assert_eq!(accepted.outcome(), &CommandResponseOutcome::Accepted);

        let rejection = CommandRejection::new(
            CommandRejectionClassification::Conflict,
            ApplicationErrorCode::new("orders.already_placed").unwrap(),
            "order was already placed",
            Some(serde_json::json!({"order_id": "one"})),
        )
        .unwrap();
        let rejected = response(CommandResponseOutcome::Rejected(rejection));
        let encoded = serde_json::to_vec(&rejected).unwrap();
        let decoded: CommandResponse = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, rejected);
    }

    #[test]
    fn response_deserialization_revalidates_rejections_and_envelope_size() {
        let invalid = serde_json::json!({
            "message_id": "response-1",
            "command_message_id": "command-1",
            "command_address": "acme.command.orders.place-order",
            "operation_id": "operation-1",
            "schema_version": 1,
            "correlation_id": "correlation-1",
            "outcome": {
                "status": "rejected",
                "value": {
                    "classification": "invalid_request",
                    "code": "bad code",
                    "message": "invalid request"
                }
            }
        });
        assert!(serde_json::from_value::<CommandResponse>(invalid).is_err());

        let invalid_schema = serde_json::json!({
            "message_id": "response-1",
            "command_message_id": "command-1",
            "command_address": "acme.command.orders.place-order",
            "operation_id": "operation-1",
            "schema_version": COMMAND_RESPONSE_SCHEMA_VERSION + 1,
            "correlation_id": "correlation-1",
            "outcome": { "status": "accepted" }
        });
        assert!(serde_json::from_value::<CommandResponse>(invalid_schema).is_err());

        let rejection = CommandRejection::new(
            CommandRejectionClassification::Internal,
            ApplicationErrorCode::new("internal.large_details").unwrap(),
            "response details are too large",
            Some(serde_json::json!({"value": "x".repeat(MAX_ENVELOPE_BYTES)})),
        )
        .unwrap();
        let error = CommandResponse::rejected(
            MessageId::new("response-1").unwrap(),
            MessageId::new("command-1").unwrap(),
            CommandAddress::new("acme", "orders", "place-order").unwrap(),
            OperationId::new("operation-1").unwrap(),
            CorrelationId::new("correlation-1").unwrap(),
            rejection,
        )
        .unwrap_err();
        assert_eq!(error.kind(), MessageBuildErrorKind::PayloadTooLarge);
    }

    #[test]
    fn rejection_messages_are_bounded_and_control_free() {
        let code = ApplicationErrorCode::new("orders.invalid").unwrap();
        assert!(
            CommandRejection::new(
                CommandRejectionClassification::InvalidRequest,
                code.clone(),
                "",
                None,
            )
            .is_err()
        );
        assert!(
            CommandRejection::new(
                CommandRejectionClassification::InvalidRequest,
                code.clone(),
                "line\nbreak",
                None,
            )
            .is_err()
        );
        assert!(
            CommandRejection::new(
                CommandRejectionClassification::InvalidRequest,
                code,
                "x".repeat(MAX_COMMAND_REJECTION_MESSAGE_BYTES + 1),
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn response_address_derivation_is_exact_stable_and_framed() {
        let command = CommandAddress::new("acme", "orders", "place-order").unwrap();
        let operation = OperationId::new("operation-1").unwrap();
        let command_message = MessageId::new("command-1").unwrap();
        let address =
            derive_command_response_address(&command, &operation, &command_message).unwrap();

        assert_eq!(address.application(), "acme");
        assert_eq!(address.context(), "orders");
        assert_eq!(
            address.as_str(),
            "acme.command-response.orders.0d0cb197be2a6e138e30cb34bb8b735e691293cec88cb6835f1e7088c480731c"
        );
        assert_eq!(address.name().len(), 64);
        assert!(
            address
                .name()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert_eq!(
            derive_command_response_address(&command, &operation, &command_message).unwrap(),
            address
        );
        assert_ne!(
            derive_command_response_address(
                &CommandAddress::new("acme", "orders", "cancel-order").unwrap(),
                &operation,
                &command_message,
            )
            .unwrap(),
            address
        );
        assert_ne!(
            derive_command_response_address(
                &command,
                &OperationId::new("operation").unwrap(),
                &MessageId::new("1command-1").unwrap(),
            )
            .unwrap(),
            derive_command_response_address(
                &command,
                &OperationId::new("operation1").unwrap(),
                &MessageId::new("command-1").unwrap(),
            )
            .unwrap()
        );
        let test_command = CommandAddress::new_in_scope(
            "acme",
            crate::TrafficScope::Test,
            "orders",
            "place-order",
        )
        .unwrap();
        let test_address =
            derive_command_response_address(&test_command, &operation, &command_message).unwrap();
        assert_eq!(test_address.traffic_scope(), crate::TrafficScope::Test);
        assert!(
            test_address
                .as_str()
                .starts_with("acme.test.command-response.orders.")
        );
        assert_ne!(test_address, address);
    }
}
