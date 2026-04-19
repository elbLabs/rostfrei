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
    messaging_config::{MessagingTopology, StreamName},
};

pub const CONTENT_TYPE_HEADER: &str = "Content-Type";
pub const JSON_CONTENT_TYPE: &str = "application/json";
pub const TRACE_PARENT_HEADER: &str = "traceparent";
pub const TRACE_STATE_HEADER: &str = "tracestate";
pub const DEFAULT_PUBLISH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsPublishAck {
    stream: StreamName,
    sequence: u64,
    duplicate: bool,
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
        self.publish_with_ack(
            message,
            self.topology.integration_event_stream(),
            publish_timeout,
        )
        .await
    }

    pub async fn publish_command_response_with_ack(
        &self,
        message: OutboundMessage<CommandResponseAddress>,
        publish_timeout: Duration,
    ) -> Result<NatsPublishAck, NatsError> {
        if message.address().application() != self.topology.application().as_str() {
            return Err(NatsError::InvalidMessage);
        }
        let response = decode_outbound_command_response(&message)?;
        let headers = safe_headers(message.metadata(), message.trace_context());
        publish_immutable_command_response(
            &self.context,
            &message,
            &response,
            self.topology.command_response_stream(),
            headers,
            publish_timeout,
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
        if message.address().application() != self.topology.application().as_str() {
            return Err(NatsError::InvalidMessage);
        }
        let headers = safe_headers(message.metadata(), message.trace_context());
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

async fn publish_immutable_command_response(
    context: &jetstream::Context,
    message: &OutboundMessage<CommandResponseAddress>,
    response: &CommandResponse,
    expected_stream: &StreamName,
    mut headers: HeaderMap,
    publish_timeout: Duration,
) -> Result<NatsPublishAck, NatsError> {
    if publish_timeout.is_zero() {
        return Err(NatsError::Configuration);
    }

    headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);
    let publish = PublishMessage::build()
        .payload(message.payload().to_vec().into())
        .headers(headers)
        .message_id(message.message_id().as_str())
        .expected_stream(expected_stream.as_str())
        .expected_last_subject_sequence(0);
    let result = timeout(publish_timeout, async {
        let acknowledgement = context
            .send_publish(message.address().as_str().to_owned(), publish)
            .await
            .map_err(|error| classify_publish_error(&error))?;
        acknowledgement
            .await
            .map_err(|error| classify_publish_error(&error))
    })
    .await
    .map_err(|_| NatsError::PublishTimeout)?;

    match result {
        Ok(acknowledgement) if !acknowledgement.duplicate => {
            if acknowledgement.stream != expected_stream.as_str() || acknowledgement.sequence == 0 {
                return Err(NatsError::PublishExpectation);
            }
            Ok(NatsPublishAck {
                stream: expected_stream.clone(),
                sequence: acknowledgement.sequence,
                duplicate: false,
            })
        }
        Ok(_) | Err(NatsError::PublishExpectation) => {
            let sequence = verify_existing_command_response(
                context,
                expected_stream,
                message.address().as_str(),
                message.payload(),
                message.message_id().as_str(),
                response,
                publish_timeout,
            )
            .await?;
            Ok(NatsPublishAck {
                stream: expected_stream.clone(),
                sequence,
                duplicate: true,
            })
        }
        Err(error) => Err(error),
    }
}

async fn verify_existing_command_response(
    context: &jetstream::Context,
    expected_stream: &StreamName,
    subject: &str,
    payload: &[u8],
    message_id: &str,
    response: &CommandResponse,
    operation_timeout: Duration,
) -> Result<u64, NatsError> {
    timeout(operation_timeout, async {
        let stream = context
            .get_stream(expected_stream.as_str())
            .await
            .map_err(|_| NatsError::StreamNotFound)?;
        let stored = stream
            .get_last_raw_message_by_subject(subject)
            .await
            .map_err(|error| {
                if matches!(
                    error.kind(),
                    jetstream::stream::LastRawMessageErrorKind::NoMessageFound
                ) {
                    NatsError::IdentityConflict
                } else {
                    NatsError::Publish
                }
            })?;
        if stored.subject.as_str() != subject
            || stored.sequence == 0
            || stored.payload.as_ref() != payload
            || one_optional_header(&stored.headers, CONTENT_TYPE_HEADER)? != Some(JSON_CONTENT_TYPE)
            || one_optional_header(&stored.headers, "Nats-Msg-Id")? != Some(message_id)
        {
            return Err(NatsError::IdentityConflict);
        }
        let stored_response: CommandResponse =
            serde_json::from_slice(&stored.payload).map_err(|_| NatsError::IdentityConflict)?;
        if stored_response.message_id().as_str() != message_id || &stored_response != response {
            return Err(NatsError::IdentityConflict);
        }
        Ok(stored.sequence)
    })
    .await
    .map_err(|_| NatsError::PublishTimeout)?
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

fn one_optional_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a str>, NatsError> {
    let mut values = headers.get_all(name.to_owned());
    let first = values.next().map(async_nats::HeaderValue::as_str);
    if values.next().is_some() {
        return Err(NatsError::IdentityConflict);
    }
    Ok(first)
}

const fn core_publish_error(error: &NatsError) -> PublishError {
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

fn classify_publish_error(error: &JetStreamPublishError) -> NatsError {
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
