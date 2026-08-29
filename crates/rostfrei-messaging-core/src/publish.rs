use async_trait::async_trait;
use serde::Serialize;

use crate::{
    CallerMetadata, CommandAddress, CommandResponseAddress, IntegrationEventAddress,
    MessageBuildError, MessageId, PublishError, PublishableAddress, TraceContext,
};

pub const MAX_MESSAGE_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundMessage<A>
where
    A: PublishableAddress,
{
    address: A,
    message_id: MessageId,
    payload: Vec<u8>,
    metadata: CallerMetadata,
    trace_context: Option<TraceContext>,
}

impl<A> OutboundMessage<A>
where
    A: PublishableAddress,
{
    pub fn new(
        address: A,
        message_id: MessageId,
        payload: Vec<u8>,
    ) -> Result<Self, MessageBuildError> {
        Self::new_with_maximum_payload_bytes(
            address,
            message_id,
            payload,
            MAX_MESSAGE_PAYLOAD_BYTES,
        )
    }

    pub fn new_with_maximum_payload_bytes(
        address: A,
        message_id: MessageId,
        payload: Vec<u8>,
        maximum: usize,
    ) -> Result<Self, MessageBuildError> {
        validate_payload_size(payload.len(), maximum)?;
        Ok(Self {
            address,
            message_id,
            payload,
            metadata: CallerMetadata::new(),
            trace_context: None,
        })
    }

    pub fn json<T>(
        address: A,
        message_id: MessageId,
        payload: &T,
    ) -> Result<Self, MessageBuildError>
    where
        T: Serialize + ?Sized,
    {
        Self::json_with_maximum_payload_bytes(
            address,
            message_id,
            payload,
            MAX_MESSAGE_PAYLOAD_BYTES,
        )
    }

    pub fn json_with_maximum_payload_bytes<T>(
        address: A,
        message_id: MessageId,
        payload: &T,
        maximum: usize,
    ) -> Result<Self, MessageBuildError>
    where
        T: Serialize + ?Sized,
    {
        if maximum == 0 || maximum > MAX_MESSAGE_PAYLOAD_BYTES {
            return Err(MessageBuildError::invalid_maximum(maximum));
        }
        let payload =
            serde_json::to_vec(payload).map_err(|_| MessageBuildError::serialization())?;
        Self::new_with_maximum_payload_bytes(address, message_id, payload, maximum)
    }

    #[must_use]
    pub fn with_metadata(mut self, metadata: CallerMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    #[must_use]
    pub fn with_trace_context(mut self, trace_context: TraceContext) -> Self {
        self.trace_context = Some(trace_context);
        self
    }

    pub const fn address(&self) -> &A {
        &self.address
    }

    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub const fn metadata(&self) -> &CallerMetadata {
        &self.metadata
    }

    pub const fn trace_context(&self) -> Option<&TraceContext> {
        self.trace_context.as_ref()
    }

    pub fn into_payload(self) -> Vec<u8> {
        self.payload
    }
}

const fn validate_payload_size(actual: usize, maximum: usize) -> Result<(), MessageBuildError> {
    if maximum == 0 || maximum > MAX_MESSAGE_PAYLOAD_BYTES {
        return Err(MessageBuildError::invalid_maximum(maximum));
    }
    if actual > maximum {
        return Err(MessageBuildError::payload_too_large(actual, maximum));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishReceipt {
    duplicate: bool,
}

impl PublishReceipt {
    pub const fn new(duplicate: bool) -> Self {
        Self { duplicate }
    }

    pub const fn duplicate(self) -> bool {
        self.duplicate
    }
}

#[async_trait]
pub trait CommandPublisher: Send + Sync {
    async fn publish_command(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<PublishReceipt, PublishError>;
}

#[async_trait]
pub trait CommandResponsePublisher: Send + Sync {
    async fn publish_command_response(
        &self,
        message: OutboundMessage<CommandResponseAddress>,
    ) -> Result<PublishReceipt, PublishError>;
}

#[async_trait]
pub trait IntegrationEventPublisher: Send + Sync {
    async fn publish_integration_event(
        &self,
        message: OutboundMessage<IntegrationEventAddress>,
    ) -> Result<PublishReceipt, PublishError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContractErrorKind, MessageBuildErrorKind};

    #[test]
    fn outbound_json_is_bounded_and_carries_only_safe_caller_context() {
        let address = IntegrationEventAddress::new("acme", "orders", "order-placed").unwrap();
        let mut metadata = CallerMetadata::new();
        metadata.insert("x-tenant", "acme").unwrap();
        let message = OutboundMessage::json(
            address,
            MessageId::new("message-1").unwrap(),
            &serde_json::json!({"order_id": "one"}),
        )
        .unwrap()
        .with_metadata(metadata);

        assert_eq!(message.payload(), br#"{"order_id":"one"}"#);
        assert_eq!(message.metadata().get("x-tenant"), Some("acme"));
        assert_eq!(
            message.address().as_str(),
            "acme.integration.orders.order-placed"
        );
    }

    #[test]
    fn caller_cannot_select_an_unbounded_payload_maximum() {
        let address = CommandAddress::new("acme", "orders", "place-order").unwrap();
        let error = OutboundMessage::json_with_maximum_payload_bytes(
            address,
            MessageId::new("message-1").unwrap(),
            &serde_json::json!({"ok": true}),
            MAX_MESSAGE_PAYLOAD_BYTES + 1,
        )
        .unwrap_err();
        assert_eq!(error.kind(), MessageBuildErrorKind::InvalidMaximum);

        let mut metadata = CallerMetadata::new();
        assert_eq!(
            metadata
                .insert("Nats-Msg-Id", "override")
                .unwrap_err()
                .kind(),
            ContractErrorKind::Reserved
        );
    }
}
