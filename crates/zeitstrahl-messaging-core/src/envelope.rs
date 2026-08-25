use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

use crate::{
    CausationId, CorrelationId, MessageBuildError, MessageId, MessageTimestamp, OperationId,
    SchemaVersion,
};

pub const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvelopeContext {
    message_id: MessageId,
    schema_version: SchemaVersion,
    correlation_id: CorrelationId,
    causation_id: Option<CausationId>,
}

impl EnvelopeContext {
    pub const fn new(
        message_id: MessageId,
        schema_version: SchemaVersion,
        correlation_id: CorrelationId,
        causation_id: Option<CausationId>,
    ) -> Self {
        Self {
            message_id,
            schema_version,
            correlation_id,
            causation_id,
        }
    }

    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (MessageId, SchemaVersion, CorrelationId, Option<CausationId>) {
        (
            self.message_id,
            self.schema_version,
            self.correlation_id,
            self.causation_id,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandEnvelope<T> {
    message_id: MessageId,
    operation_id: OperationId,
    schema_version: SchemaVersion,
    created_at: MessageTimestamp,
    correlation_id: CorrelationId,
    #[serde(skip_serializing_if = "Option::is_none")]
    causation_id: Option<CausationId>,
    payload: T,
}

impl<T> CommandEnvelope<T>
where
    T: Serialize,
{
    pub fn new(
        context: EnvelopeContext,
        operation_id: OperationId,
        created_at: MessageTimestamp,
        payload: T,
    ) -> Result<Self, MessageBuildError> {
        let envelope = Self {
            message_id: context.message_id,
            operation_id,
            schema_version: context.schema_version,
            created_at,
            correlation_id: context.correlation_id,
            causation_id: context.causation_id,
            payload,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), MessageBuildError> {
        validate_serialized_size(self)
    }
}

impl<T> CommandEnvelope<T> {
    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub const fn created_at(&self) -> MessageTimestamp {
        self.created_at
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub const fn payload(&self) -> &T {
        &self.payload
    }

    pub fn into_payload(self) -> T {
        self.payload
    }
}

#[derive(Deserialize)]
struct CommandEnvelopeWire<T> {
    message_id: MessageId,
    operation_id: OperationId,
    schema_version: SchemaVersion,
    created_at: MessageTimestamp,
    correlation_id: CorrelationId,
    causation_id: Option<CausationId>,
    payload: T,
}

impl<'de, T> Deserialize<'de> for CommandEnvelope<T>
where
    T: Deserialize<'de> + Serialize,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CommandEnvelopeWire::deserialize(deserializer)?;
        Self::new(
            EnvelopeContext::new(
                wire.message_id,
                wire.schema_version,
                wire.correlation_id,
                wire.causation_id,
            ),
            wire.operation_id,
            wire.created_at,
            wire.payload,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntegrationEventEnvelope<T> {
    message_id: MessageId,
    schema_version: SchemaVersion,
    occurred_at: MessageTimestamp,
    correlation_id: CorrelationId,
    #[serde(skip_serializing_if = "Option::is_none")]
    causation_id: Option<CausationId>,
    payload: T,
}

impl<T> IntegrationEventEnvelope<T>
where
    T: Serialize,
{
    pub fn new(
        context: EnvelopeContext,
        occurred_at: MessageTimestamp,
        payload: T,
    ) -> Result<Self, MessageBuildError> {
        let envelope = Self {
            message_id: context.message_id,
            schema_version: context.schema_version,
            occurred_at,
            correlation_id: context.correlation_id,
            causation_id: context.causation_id,
            payload,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), MessageBuildError> {
        validate_serialized_size(self)
    }
}

impl<T> IntegrationEventEnvelope<T> {
    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub const fn occurred_at(&self) -> MessageTimestamp {
        self.occurred_at
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub const fn payload(&self) -> &T {
        &self.payload
    }

    pub fn into_payload(self) -> T {
        self.payload
    }
}

#[derive(Deserialize)]
struct IntegrationEventEnvelopeWire<T> {
    message_id: MessageId,
    schema_version: SchemaVersion,
    occurred_at: MessageTimestamp,
    correlation_id: CorrelationId,
    causation_id: Option<CausationId>,
    payload: T,
}

impl<'de, T> Deserialize<'de> for IntegrationEventEnvelope<T>
where
    T: Deserialize<'de> + Serialize,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = IntegrationEventEnvelopeWire::deserialize(deserializer)?;
        Self::new(
            EnvelopeContext::new(
                wire.message_id,
                wire.schema_version,
                wire.correlation_id,
                wire.causation_id,
            ),
            wire.occurred_at,
            wire.payload,
        )
        .map_err(D::Error::custom)
    }
}

pub(crate) fn validate_serialized_size<T>(value: &T) -> Result<(), MessageBuildError>
where
    T: Serialize + ?Sized,
{
    let size = serde_json::to_vec(value)
        .map_err(|_| MessageBuildError::serialization())?
        .len();
    if size > MAX_ENVELOPE_BYTES {
        return Err(MessageBuildError::payload_too_large(
            size,
            MAX_ENVELOPE_BYTES,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MessageBuildErrorKind, MAX_ENVELOPE_BYTES};

    fn context() -> EnvelopeContext {
        EnvelopeContext::new(
            MessageId::new("message-1").unwrap(),
            SchemaVersion::new(2).unwrap(),
            CorrelationId::new("correlation-1").unwrap(),
            Some(CausationId::new("cause-1").unwrap()),
        )
    }

    #[test]
    fn command_envelopes_preserve_operation_and_creation_context() {
        let envelope = CommandEnvelope::new(
            context(),
            OperationId::new("operation-1").unwrap(),
            MessageTimestamp::from_unix_milliseconds(1_700_000_000_000).unwrap(),
            serde_json::json!({"order_id": "one"}),
        )
        .unwrap();

        assert_eq!(envelope.message_id().as_str(), "message-1");
        assert_eq!(envelope.operation_id().as_str(), "operation-1");
        assert_eq!(envelope.schema_version().get(), 2);
        assert_eq!(envelope.created_at().unix_milliseconds(), 1_700_000_000_000);
        assert_eq!(envelope.causation_id().unwrap().as_str(), "cause-1");

        let encoded = serde_json::to_vec(&envelope).unwrap();
        let decoded: CommandEnvelope<serde_json::Value> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, envelope);
    }

    #[test]
    fn integration_event_envelopes_preserve_occurrence_time() {
        let occurred_at = MessageTimestamp::from_unix_milliseconds(1_700_000_000_001).unwrap();
        let envelope = IntegrationEventEnvelope::new(
            context(),
            occurred_at,
            serde_json::json!({"order_id": "one"}),
        )
        .unwrap();
        assert_eq!(envelope.occurred_at(), occurred_at);
    }

    #[test]
    fn envelopes_are_bounded_before_transport_selection() {
        let error = IntegrationEventEnvelope::new(
            context(),
            MessageTimestamp::from_unix_milliseconds(1).unwrap(),
            "x".repeat(MAX_ENVELOPE_BYTES),
        )
        .unwrap_err();
        assert_eq!(error.kind(), MessageBuildErrorKind::PayloadTooLarge);
        assert!(error.actual().unwrap() > MAX_ENVELOPE_BYTES);
    }
}
