use std::{error::Error as _, time::Duration};

use async_nats::{
    HeaderMap,
    jetstream::{
        self,
        context::{PublishError as JetStreamPublishError, PublishErrorKind},
        message::PublishMessage,
    },
};
use async_trait::async_trait;
use rostfrei_messaging_core::{
    CallerMetadata, CommandAddress, CommandPublisher, CommandResponse, CommandResponseAddress,
    CommandResponsePublisher, IntegrationEventAddress, IntegrationEventPublisher, OutboundMessage,
    PublishError, PublishErrorKind as CorePublishErrorKind, PublishReceipt, PublishableAddress,
    TraceContext, derive_command_response_address,
};
use tokio::time::timeout;

use crate::{
    error::NatsError,
    integration_subject::{IntegrationEventSubject, outbound_envelope},
    messaging_config::{MessagingTopology, StreamName},
};

pub const CONTENT_TYPE_HEADER: &str = "Content-Type";
pub const JSON_CONTENT_TYPE: &str = "application/json";
pub const CORRELATION_ID_HEADER: &str = "rostfrei-Control-Correlation-Id";
pub const TRACE_PARENT_HEADER: &str = "traceparent";
pub const TRACE_STATE_HEADER: &str = "tracestate";
pub const DEFAULT_PUBLISH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsPublishAck {
    pub(crate) stream: StreamName,
    pub(crate) sequence: u64,
    pub(crate) duplicate: bool,
}

impl NatsPublishAck {
    pub const fn stream(&self) -> &StreamName {
        &self.stream
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn duplicate(&self) -> bool {
        self.duplicate
    }
}

#[derive(Clone)]
pub struct NatsPublisher {
    context: jetstream::Context,
    topology: MessagingTopology,
    publish_timeout: Duration,
}

impl NatsPublisher {
    pub const fn new(context: jetstream::Context, topology: MessagingTopology) -> Self {
        Self {
            context,
            topology,
            publish_timeout: DEFAULT_PUBLISH_TIMEOUT,
        }
    }

    pub fn with_publish_timeout(mut self, publish_timeout: Duration) -> Result<Self, NatsError> {
        if publish_timeout.is_zero() {
            return Err(NatsError::Configuration);
        }
        self.publish_timeout = publish_timeout;
        Ok(self)
    }

    pub const fn publish_timeout(&self) -> Duration {
        self.publish_timeout
    }

    pub async fn publish_command_with_ack(
        &self,
        message: OutboundMessage<CommandAddress>,
        publish_timeout: Duration,
    ) -> Result<NatsPublishAck, NatsError> {
        self.publish_with_ack(message, self.topology.command_stream(), publish_timeout)
            .await
    }

    pub async fn publish_integration_event_with_ack(
        &self,
        message: OutboundMessage<IntegrationEventAddress>,
        publish_timeout: Duration,
    ) -> Result<NatsPublishAck, NatsError> {
        if message.address().application() != self.topology.application().as_str()
            || message.address().traffic_scope() != self.topology.traffic_scope()
        {
            return Err(NatsError::InvalidMessage);
        }
        let envelope = outbound_envelope(&message)?;
        let subject = IntegrationEventSubject::new(
            message.address().clone(),
            envelope.correlation_id().clone(),
            envelope.causation_id().cloned(),
        )?;
        let mut headers = safe_headers(message.metadata(), message.trace_context());
        headers.insert(CORRELATION_ID_HEADER, envelope.correlation_id().as_str());
        publish_confirmed(
            &self.context,
            subject.as_str(),
            message.payload(),
            message.message_id().as_str(),
            self.topology.integration_event_stream(),
            headers,
            publish_timeout,
        )
        .await
    }

    pub async fn publish_command_response_with_ack(
        &self,
        message: OutboundMessage<CommandResponseAddress>,
        publish_timeout: Duration,
    ) -> Result<NatsPublishAck, NatsError> {
        // This convenience call is a fresh snapshot/upsert. Reprepare only after a definite
        // CAS conflict; uncertain delivery returns to the caller without changing its token.
        // Callers performing uncertain retries can retain PreparedCommandResponse explicitly.
        for _ in 0..3 {
            let prepared = self
                .prepare_command_response(message.clone(), publish_timeout)
                .await?;
            match self
                .publish_prepared_command_response(&prepared, publish_timeout)
                .await
            {
                Err(NatsError::PublishExpectation) => {}
                result => return result,
            }
        }
        Err(NatsError::PublishExpectation)
    }

    pub async fn prepare_command_response(
        &self,
        message: OutboundMessage<CommandResponseAddress>,
        operation_timeout: Duration,
    ) -> Result<crate::response_publication::PreparedCommandResponse, NatsError> {
        if message.address().application() != self.topology.application().as_str()
            || message.address().traffic_scope() != self.topology.traffic_scope()
        {
            return Err(NatsError::InvalidMessage);
        }
        let response = decode_outbound_command_response(&message)?;
        if message
            .correlation_id()
            .is_some_and(|correlation_id| correlation_id != response.correlation_id())
        {
            return Err(NatsError::InvalidMessage);
        }
        let mut headers = safe_headers(message.metadata(), message.trace_context());
        headers.insert(CORRELATION_ID_HEADER, response.correlation_id().as_str());
        crate::response_publication::prepare(
            &self.context,
            message,
            response,
            self.topology.command_response_stream(),
            headers,
            operation_timeout,
        )
        .await
    }

    /// Retry the same prepared value after uncertainty. A stale/superseded generation is
    /// refused rather than being silently rebased; re-evaluate before preparing a new snapshot.
    pub async fn publish_prepared_command_response(
        &self,
        prepared: &crate::response_publication::PreparedCommandResponse,
        operation_timeout: Duration,
    ) -> Result<NatsPublishAck, NatsError> {
        if prepared.message().address().application() != self.topology.application().as_str()
            || prepared.message().address().traffic_scope() != self.topology.traffic_scope()
        {
            return Err(NatsError::InvalidMessage);
        }
        crate::response_publication::publish(
            &self.context,
            prepared,
            self.topology.command_response_stream(),
            operation_timeout,
        )
        .await
    }

    async fn publish_with_ack<A>(
        &self,
        message: OutboundMessage<A>,
        expected_stream: &StreamName,
        publish_timeout: Duration,
    ) -> Result<NatsPublishAck, NatsError>
    where
        A: PublishableAddress,
    {
        if message.address().application() != self.topology.application().as_str()
            || message.address().traffic_scope() != self.topology.traffic_scope()
        {
            return Err(NatsError::InvalidMessage);
        }
        let mut headers = safe_headers(message.metadata(), message.trace_context());
        if let Some(correlation_id) = message.correlation_id() {
            headers.insert(CORRELATION_ID_HEADER, correlation_id.as_str());
        }
        publish_confirmed(
            &self.context,
            message.address().as_str(),
            message.payload(),
            message.message_id().as_str(),
            expected_stream,
            headers,
            publish_timeout,
        )
        .await
    }

    pub async fn flush(&self) -> Result<(), NatsError> {
        self.context
            .client()
            .flush()
            .await
            .map_err(|_| NatsError::Flush)
    }
}

#[async_trait]
impl CommandPublisher for NatsPublisher {
    async fn publish_command(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        self.publish_command_with_ack(message, self.publish_timeout)
            .await
            .map(|ack| PublishReceipt::new(ack.duplicate()))
            .map_err(|error| core_publish_error(&error))
    }
}

#[async_trait]
impl CommandResponsePublisher for NatsPublisher {
    async fn publish_command_response(
        &self,
        message: OutboundMessage<CommandResponseAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        self.publish_command_response_with_ack(message, self.publish_timeout)
            .await
            .map(|ack| PublishReceipt::new(ack.duplicate()))
            .map_err(|error| core_publish_error(&error))
    }
}

#[async_trait]
impl IntegrationEventPublisher for NatsPublisher {
    async fn publish_integration_event(
        &self,
        message: OutboundMessage<IntegrationEventAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        self.publish_integration_event_with_ack(message, self.publish_timeout)
            .await
            .map(|ack| PublishReceipt::new(ack.duplicate()))
            .map_err(|error| core_publish_error(&error))
    }
}

pub fn safe_headers(metadata: &CallerMetadata, trace_context: Option<&TraceContext>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in metadata.iter() {
        headers.insert(name.to_owned(), value.to_owned());
    }
    if let Some(trace_context) = trace_context {
        headers.insert(TRACE_PARENT_HEADER, trace_context.trace_parent());
        if let Some(trace_state) = trace_context.trace_state() {
            headers.insert(TRACE_STATE_HEADER, trace_state);
        }
    }
    headers
}

pub async fn publish_confirmed(
    context: &jetstream::Context,
    subject: &str,
    payload: &[u8],
    message_id: &str,
    expected_stream: &StreamName,
    mut headers: HeaderMap,
    publish_timeout: Duration,
) -> Result<NatsPublishAck, NatsError> {
    if publish_timeout.is_zero() {
        return Err(NatsError::Configuration);
    }

    // Controls are inserted last so no caller-provided value can survive a collision.
    headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);
    let publish = PublishMessage::build()
        .payload(payload.to_vec().into())
        .headers(headers)
        .message_id(message_id)
        .expected_stream(expected_stream.as_str());

    let acknowledgement = timeout(publish_timeout, async {
        let acknowledgement = context
            .send_publish(subject.to_owned(), publish)
            .await
            .map_err(|error| classify_publish_error(&error))?;
        acknowledgement
            .await
            .map_err(|error| classify_publish_error(&error))
    })
    .await
    .map_err(|_| NatsError::PublishTimeout)??;

    if acknowledgement.stream != expected_stream.as_str() || acknowledgement.sequence == 0 {
        return Err(NatsError::PublishExpectation);
    }

    Ok(NatsPublishAck {
        stream: expected_stream.clone(),
        sequence: acknowledgement.sequence,
        duplicate: acknowledgement.duplicate,
    })
}

fn decode_outbound_command_response(
    message: &OutboundMessage<CommandResponseAddress>,
) -> Result<CommandResponse, NatsError> {
    let response: CommandResponse =
        serde_json::from_slice(message.payload()).map_err(|_| NatsError::InvalidMessage)?;
    let expected_address = derive_command_response_address(
        response.command_address(),
        response.operation_id(),
        response.command_message_id(),
    )
    .map_err(|_| NatsError::InvalidMessage)?;
    if response.message_id() != message.message_id() || &expected_address != message.address() {
        return Err(NatsError::InvalidMessage);
    }
    Ok(response)
}

pub const fn core_publish_error(error: &NatsError) -> PublishError {
    let kind = match error {
        NatsError::Configuration => CorePublishErrorKind::InvalidConfiguration,
        NatsError::PublishTimeout => CorePublishErrorKind::Timeout,
        NatsError::PayloadTooLarge { .. }
        | NatsError::MessageTooLarge
        | NatsError::PublishExpectation
        | NatsError::IdentityConflict
        | NatsError::InvalidMessage => CorePublishErrorKind::Rejected,
        _ => CorePublishErrorKind::Unavailable,
    };
    PublishError::new(kind)
}

pub fn classify_publish_error(error: &JetStreamPublishError) -> NatsError {
    match error.kind() {
        PublishErrorKind::TimedOut => NatsError::PublishTimeout,
        PublishErrorKind::WrongLastMessageId | PublishErrorKind::WrongLastSequence => {
            NatsError::PublishExpectation
        }
        PublishErrorKind::StreamNotFound => NatsError::StreamNotFound,
        PublishErrorKind::MaxPayloadExceeded => NatsError::MessageTooLarge,
        PublishErrorKind::Other if is_publish_expectation_error(error) => {
            NatsError::PublishExpectation
        }
        PublishErrorKind::Other if is_message_too_large_error(error) => NatsError::MessageTooLarge,
        PublishErrorKind::BrokenPipe
        | PublishErrorKind::MaxAckPending
        | PublishErrorKind::Other => NatsError::Publish,
    }
}

fn publish_api_error_code(
    error: &JetStreamPublishError,
) -> Option<async_nats::jetstream::ErrorCode> {
    error
        .source()?
        .downcast_ref::<async_nats::jetstream::Error>()
        .map(async_nats::jetstream::Error::error_code)
}

fn is_publish_expectation_error(error: &JetStreamPublishError) -> bool {
    use async_nats::jetstream::ErrorCode;

    matches!(
        publish_api_error_code(error),
        Some(
            ErrorCode::STREAM_MISMATCH
                | ErrorCode::STREAM_NOT_MATCH
                | ErrorCode::STREAM_SEQUENCE_NOT_MATCH
        )
    )
}

fn is_message_too_large_error(error: &JetStreamPublishError) -> bool {
    publish_api_error_code(error)
        == Some(async_nats::jetstream::ErrorCode::STREAM_MESSAGE_EXCEEDS_MAXIMUM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rostfrei_messaging_core::{
        CorrelationId, MessageId, OperationId, derive_command_response_address,
    };

    const CORE_TIMEOUT_ERROR: PublishError = core_publish_error(&NatsError::PublishTimeout);

    #[test]
    fn outbound_command_response_identity_must_match_payload() {
        let command = CommandAddress::new("acme", "orders", "place-order").unwrap();
        let operation = OperationId::new("operation-1").unwrap();
        let command_message_id = MessageId::new("command-1").unwrap();
        let address =
            derive_command_response_address(&command, &operation, &command_message_id).unwrap();
        let response = CommandResponse::accepted(
            MessageId::new("response-1").unwrap(),
            command_message_id,
            command,
            operation,
            CorrelationId::new("correlation-1").unwrap(),
        )
        .unwrap();
        let valid =
            OutboundMessage::json(address.clone(), response.message_id().clone(), &response)
                .unwrap();
        assert_eq!(decode_outbound_command_response(&valid).unwrap(), response);

        let mismatched_address = derive_command_response_address(
            &CommandAddress::new("acme", "orders", "cancel-order").unwrap(),
            response.operation_id(),
            response.command_message_id(),
        )
        .unwrap();
        let mismatched_subject =
            OutboundMessage::json(mismatched_address, response.message_id().clone(), &response)
                .unwrap();
        assert_eq!(
            decode_outbound_command_response(&mismatched_subject).unwrap_err(),
            NatsError::InvalidMessage
        );

        let invalid = OutboundMessage::json(
            address,
            MessageId::new("different-response").unwrap(),
            &response,
        )
        .unwrap();
        assert_eq!(
            decode_outbound_command_response(&invalid).unwrap_err(),
            NatsError::InvalidMessage
        );
    }

    #[test]
    fn core_publish_errors_are_const_mapped() {
        assert_eq!(CORE_TIMEOUT_ERROR.kind(), CorePublishErrorKind::Timeout);
    }
}
