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
    CallerMetadata, CommandAddress, CommandPublisher, IntegrationEventAddress,
    IntegrationEventPublisher, OutboundMessage, PublishError,
    PublishErrorKind as CorePublishErrorKind, PublishReceipt, PublishableAddress, TraceContext,
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

pub(crate) fn safe_headers(
    metadata: &CallerMetadata,
    trace_context: Option<&TraceContext>,
) -> HeaderMap {
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

pub(crate) async fn publish_confirmed(
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

fn core_publish_error(error: &NatsError) -> PublishError {
    let kind = match error {
        NatsError::Configuration => CorePublishErrorKind::InvalidConfiguration,
        NatsError::PublishTimeout => CorePublishErrorKind::Timeout,
        NatsError::PayloadTooLarge { .. }
        | NatsError::MessageTooLarge
        | NatsError::PublishExpectation
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
