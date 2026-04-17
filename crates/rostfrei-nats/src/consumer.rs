use std::{fmt::Write as _, sync::Arc, time::Duration};

use async_nats::{
    HeaderMap,
    jetstream::{
        self,
        consumer::{self, AckPolicy, DeliverPolicy},
        message::AckKind,
    },
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::TryStreamExt;
use rostfrei_messaging_core::{
    CallerMetadata, CommandAddress, ConsumeError, ConsumeErrorKind, ConsumerConfig,
    DeliveryDisposition, DeliveryInfo, IntegrationEventAddress, MessageAddress, MessageConsumer,
    MessageConsumerFactory, MessageDelivery, MessageHandler, MessageId, PublishableAddress,
    TraceContext,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::{Instant, sleep, timeout};

use crate::{
    error::NatsError,
    messaging_config::{MessagingTopology, StreamName},
    provisioning::durable_consumer_config,
    publish::{
        CONTENT_TYPE_HEADER, DEFAULT_PUBLISH_TIMEOUT, JSON_CONTENT_TYPE, TRACE_PARENT_HEADER,
        TRACE_STATE_HEADER, publish_confirmed, safe_headers,
    },
};

pub const MAX_QUARANTINE_RECORD_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_QUARANTINE_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_QUARANTINE_PAYLOAD_PREFIX_BYTES: usize = 16 * 1024;
const MAX_QUARANTINE_PAYLOAD_PREFIX_BASE64_BYTES: usize =
    (MAX_QUARANTINE_PAYLOAD_PREFIX_BYTES / 3) * 4;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuarantineRecord {
    message_id: String,
    address: String,
    payload_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_size: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_sha256: Option<String>,
    #[serde(default)]
    payload_truncated: bool,
    metadata: CallerMetadata,
    trace_context: Option<TraceContext>,
    reason: String,
    attempt: u32,
    pending: u64,
    source_sequence: u64,
    consumer_sequence: u64,
    source_stream: String,
    source_consumer: String,
}

impl QuarantineRecord {
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn payload_base64(&self) -> &str {
        &self.payload_base64
    }

    pub const fn payload_size(&self) -> Option<usize> {
        self.payload_size
    }

    pub fn payload_sha256(&self) -> Option<&str> {
        self.payload_sha256.as_deref()
    }

    pub const fn payload_truncated(&self) -> bool {
        self.payload_truncated
    }

    pub const fn metadata(&self) -> &CallerMetadata {
        &self.metadata
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub const fn attempt(&self) -> u32 {
        self.attempt
    }

    pub const fn source_sequence(&self) -> u64 {
        self.source_sequence
    }

    pub const fn consumer_sequence(&self) -> u64 {
        self.consumer_sequence
    }
}

#[derive(Clone)]
pub struct NatsConsumerFactory {
    context: jetstream::Context,
    topology: MessagingTopology,
    quarantine_publish_timeout: Duration,
}

impl NatsConsumerFactory {
    pub const fn new(context: jetstream::Context, topology: MessagingTopology) -> Self {
        Self {
            context,
            topology,
            quarantine_publish_timeout: DEFAULT_PUBLISH_TIMEOUT,
        }
    }

    pub fn with_quarantine_publish_timeout(
        mut self,
        quarantine_publish_timeout: Duration,
    ) -> Result<Self, NatsError> {
        if quarantine_publish_timeout.is_zero() {
            return Err(NatsError::Configuration);
        }
        self.quarantine_publish_timeout = quarantine_publish_timeout;
        Ok(self)
    }

    fn create_consumer<A>(
        &self,
        config: ConsumerConfig<A>,
    ) -> Result<Arc<dyn MessageConsumer<A>>, ConsumeError>
    where
        A: ConsumableAddress,
    {
        if config.address().application() != self.topology.application().as_str() {
            return Err(ConsumeError::new(ConsumeErrorKind::InvalidConfiguration));
        }
        i64::try_from(config.concurrency())
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::InvalidConfiguration))?;
        let source_stream = self
            .topology
            .stream_for(config.address().kind())
            .ok_or_else(|| ConsumeError::new(ConsumeErrorKind::InvalidConfiguration))?
            .clone();
        Ok(Arc::new(NatsConsumer {
            context: self.context.clone(),
            source_stream,
            quarantine_stream: self.topology.quarantine_stream().clone(),
            quarantine_publish_timeout: self.quarantine_publish_timeout,
            config,
        }))
    }
}

impl MessageConsumerFactory<CommandAddress> for NatsConsumerFactory {
    fn create(
        &self,
        config: ConsumerConfig<CommandAddress>,
    ) -> Result<Arc<dyn MessageConsumer<CommandAddress>>, ConsumeError> {
        self.create_consumer(config)
    }
}

impl MessageConsumerFactory<IntegrationEventAddress> for NatsConsumerFactory {
    fn create(
        &self,
        config: ConsumerConfig<IntegrationEventAddress>,
    ) -> Result<Arc<dyn MessageConsumer<IntegrationEventAddress>>, ConsumeError> {
        self.create_consumer(config)
    }
}

trait ConsumableAddress: PublishableAddress {
    fn parse_nats(value: String) -> Result<Self, NatsError>;
}

impl ConsumableAddress for CommandAddress {
    fn parse_nats(value: String) -> Result<Self, NatsError> {
        Self::parse(value).map_err(|_| NatsError::InvalidMessage)
    }
}

impl ConsumableAddress for IntegrationEventAddress {
    fn parse_nats(value: String) -> Result<Self, NatsError> {
        Self::parse(value).map_err(|_| NatsError::InvalidMessage)
    }
}

struct NatsConsumer<A>
where
    A: ConsumableAddress,
{
    context: jetstream::Context,
    source_stream: StreamName,
    quarantine_stream: StreamName,
    quarantine_publish_timeout: Duration,
    config: ConsumerConfig<A>,
}

#[async_trait]
impl<A> MessageConsumer<A> for NatsConsumer<A>
where
    A: ConsumableAddress,
{
    async fn run(&self, handler: Arc<dyn MessageHandler<A>>) -> Result<(), ConsumeError> {
        let stream = self
            .context
            .get_stream(self.source_stream.as_str())
            .await
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
        let consumer: consumer::PullConsumer = stream
            .get_consumer(self.config.durable_name().as_str())
            .await
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
        verify_consumer(&consumer, &self.config)?;

        consumer
            .stream()
            .max_messages_per_batch(self.config.concurrency())
            .messages()
            .await
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))
            .try_for_each_concurrent(self.config.concurrency(), |message| {
                process_message(
                    &self.context,
                    &self.source_stream,
                    &self.quarantine_stream,
                    self.quarantine_publish_timeout,
                    &self.config,
                    handler.clone(),
                    message,
                )
            })
            .await?;
        Err(ConsumeError::new(ConsumeErrorKind::Ended))
    }
}

fn verify_consumer<A>(
    consumer: &consumer::PullConsumer,
    config: &ConsumerConfig<A>,
) -> Result<(), ConsumeError>
where
    A: PublishableAddress,
{
    let expected = durable_consumer_config(config)
        .map_err(|_| ConsumeError::new(ConsumeErrorKind::InvalidConfiguration))?;
    let actual = &consumer.cached_info().config;
    if consumer.cached_info().name != config.durable_name().as_str()
        || actual.deliver_subject.is_some()
        || actual.durable_name.as_deref() != expected.durable_name.as_deref()
        || actual.deliver_policy != DeliverPolicy::All
        || actual.ack_policy != AckPolicy::Explicit
        || actual.ack_wait != expected.ack_wait
        || actual.max_deliver != -1
        || actual.filter_subject != expected.filter_subject
        || actual.max_ack_pending != expected.max_ack_pending
    {
        return Err(ConsumeError::new(ConsumeErrorKind::InvalidConfiguration));
    }
    Ok(())
}

async fn process_message<A>(
    context: &jetstream::Context,
    source_stream: &StreamName,
    quarantine_stream: &StreamName,
    quarantine_publish_timeout: Duration,
    config: &ConsumerConfig<A>,
    handler: Arc<dyn MessageHandler<A>>,
    message: jetstream::Message,
) -> Result<(), ConsumeError>
where
    A: ConsumableAddress,
{
    let raw_info = message
        .info()
        .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
    if raw_info.stream != source_stream.as_str()
        || raw_info.consumer != config.durable_name().as_str()
        || message.subject.as_str() != config.address().as_str()
    {
        return apply_ack(&message, AckKind::Nak(Some(DEFAULT_QUARANTINE_RETRY_DELAY))).await;
    }

    let attempt = u32::try_from(raw_info.delivered)
        .ok()
        .filter(|attempt| *attempt > 0)
        .ok_or_else(|| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
    let info = DeliveryInfo::new(
        attempt,
        raw_info.pending,
        raw_info.stream_sequence,
        raw_info.consumer_sequence,
    )
    .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;

    let Ok(delivery) = decode_delivery::<A>(&message, info, config.address()) else {
        let raw = raw_quarantine_record(
            &message,
            info,
            source_stream,
            config.durable_name().as_str(),
            "invalid source message",
        );
        return quarantine_or_nak(
            context,
            quarantine_stream,
            quarantine_publish_timeout,
            &message,
            raw,
            attempt,
            config.maximum_delivery_attempts(),
        )
        .await;
    };

    if attempt > config.maximum_delivery_attempts() {
        let record = quarantine_record(
            &delivery,
            source_stream,
            config.durable_name().as_str(),
            "maximum delivery attempts exceeded",
        );
        return quarantine_or_nak(
            context,
            quarantine_stream,
            quarantine_publish_timeout,
            &message,
            record,
            attempt,
            config.maximum_delivery_attempts(),
        )
        .await;
    }

    let disposition = handle_with_progress(config, &message, handler, delivery.clone()).await?;
    apply_disposition(
        source_stream,
        quarantine_stream,
        quarantine_publish_timeout,
        config,
        &message,
        &delivery,
        disposition,
    )
    .await
}

async fn handle_with_progress<A>(
    config: &ConsumerConfig<A>,
    message: &jetstream::Message,
    handler: Arc<dyn MessageHandler<A>>,
    delivery: MessageDelivery<A>,
) -> Result<Result<DeliveryDisposition, tokio::time::error::Elapsed>, ConsumeError>
where
    A: ConsumableAddress,
{
    send_progress(message).await?;

    let handling = timeout(config.processing_timeout(), handler.handle(delivery));
    tokio::pin!(handling);
    let progress_interval = config.ack_wait() / 2;
    let progress = sleep(progress_interval);
    tokio::pin!(progress);

    loop {
        tokio::select! {
            biased;
            disposition = &mut handling => {
                send_progress(message).await?;
                return Ok(disposition);
            }
            () = &mut progress => {
                send_progress(message).await?;
                progress.as_mut().reset(Instant::now() + progress_interval);
            }
        }
    }
}

async fn send_progress(message: &jetstream::Message) -> Result<(), ConsumeError> {
    message
        .ack_with(AckKind::Progress)
        .await
        .map_err(|_| ConsumeError::new(ConsumeErrorKind::Disposition))
}

async fn apply_disposition<A>(
    source_stream: &StreamName,
    quarantine_stream: &StreamName,
    quarantine_publish_timeout: Duration,
    config: &ConsumerConfig<A>,
    message: &jetstream::Message,
    delivery: &MessageDelivery<A>,
    disposition: Result<DeliveryDisposition, tokio::time::error::Elapsed>,
) -> Result<(), ConsumeError>
where
    A: ConsumableAddress,
{
    let attempt = delivery.attempt();
    match disposition {
        Ok(DeliveryDisposition::Acknowledge) => apply_ack(message, AckKind::Ack).await,
        Ok(DeliveryDisposition::Terminate) => apply_ack(message, AckKind::Term).await,
        Ok(DeliveryDisposition::RetryAfter(delay))
            if attempt < config.maximum_delivery_attempts() =>
        {
            apply_ack(message, AckKind::Nak(Some(delay.get()))).await
        }
        Ok(DeliveryDisposition::RetryAfter(_)) | Err(_) => {
            if attempt < config.maximum_delivery_attempts() {
                return apply_ack(message, AckKind::Nak(Some(DEFAULT_QUARANTINE_RETRY_DELAY)))
                    .await;
            }
            let record = quarantine_record(
                delivery,
                source_stream,
                config.durable_name().as_str(),
                "maximum delivery attempts exceeded",
            );
            quarantine_or_nak(
                &message.context,
                quarantine_stream,
                quarantine_publish_timeout,
                message,
                record,
                attempt,
                config.maximum_delivery_attempts(),
            )
            .await
        }
        Ok(DeliveryDisposition::Quarantine(reason)) => {
            let record = quarantine_record(
                delivery,
                source_stream,
                config.durable_name().as_str(),
                reason.as_str(),
            );
            quarantine_or_nak(
                &message.context,
                quarantine_stream,
                quarantine_publish_timeout,
                message,
                record,
                attempt,
                config.maximum_delivery_attempts(),
            )
            .await
        }
    }
}

fn decode_delivery<A>(
    message: &jetstream::Message,
    info: DeliveryInfo,
    expected_address: &A,
) -> Result<MessageDelivery<A>, NatsError>
where
    A: ConsumableAddress,
{
    let headers = message.headers.as_ref().ok_or(NatsError::InvalidMessage)?;
    if single_header(headers, CONTENT_TYPE_HEADER)? != Some(JSON_CONTENT_TYPE) {
        return Err(NatsError::InvalidMessage);
    }
    let message_id = single_header(headers, "Nats-Msg-Id")?
        .ok_or(NatsError::InvalidMessage)
        .and_then(|value| MessageId::new(value).map_err(|_| NatsError::InvalidMessage))?;
    let metadata = caller_metadata(headers)?;
    let trace_context = trace_context(headers)?;
    let address = A::parse_nats(message.subject.to_string())?;
    if &address != expected_address {
        return Err(NatsError::InvalidMessage);
    }
    MessageDelivery::new_with_trace_context(
        address,
        message_id,
        message.payload.to_vec(),
        metadata,
        trace_context,
        info,
    )
    .map_err(|_| NatsError::InvalidMessage)
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, NatsError> {
    let mut values = headers.get_all(name.to_owned());
    let first = values.next().map(async_nats::HeaderValue::as_str);
    if values.next().is_some() {
        return Err(NatsError::InvalidMessage);
    }
    Ok(first)
}

fn caller_metadata(headers: &HeaderMap) -> Result<CallerMetadata, NatsError> {
    let mut metadata = CallerMetadata::new();
    for (name, values) in headers.iter() {
        let name = name.to_string();
        if is_control_header(&name) {
            continue;
        }
        let mut values = values.iter();
        let Some(value) = values.next() else {
            return Err(NatsError::InvalidMessage);
        };
        if values.next().is_some() {
            return Err(NatsError::InvalidMessage);
        }
        metadata
            .insert(name, value.as_str())
            .map_err(|_| NatsError::InvalidMessage)?;
    }
    Ok(metadata)
}

fn trace_context(headers: &HeaderMap) -> Result<Option<TraceContext>, NatsError> {
    let trace_parent = single_header(headers, TRACE_PARENT_HEADER)?;
    let trace_state = single_header(headers, TRACE_STATE_HEADER)?;
    match (trace_parent, trace_state) {
        (None, None) => Ok(None),
        (Some(parent), state) => TraceContext::from_parts(parent, state)
            .map(Some)
            .map_err(|_| NatsError::InvalidMessage),
        (None, Some(_)) => Err(NatsError::InvalidMessage),
    }
}

fn is_control_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "content-type"
        || name == "reply"
        || name == "reply-to"
        || name == "reply-subject"
        || name == TRACE_PARENT_HEADER
        || name == TRACE_STATE_HEADER
        || name.starts_with("nats-")
        || name.starts_with("rostfrei-control-")
}

fn quarantine_record<A>(
    delivery: &MessageDelivery<A>,
    source_stream: &StreamName,
    source_consumer: &str,
    reason: &str,
) -> QuarantineRecord
where
    A: PublishableAddress,
{
    let payload = delivery.payload();
    QuarantineRecord {
        message_id: delivery.message_id().as_str().to_owned(),
        address: delivery.address().as_str().to_owned(),
        payload_base64: BASE64.encode(payload),
        payload_size: Some(payload.len()),
        payload_sha256: Some(sha256_hex(payload)),
        payload_truncated: false,
        metadata: delivery.metadata().clone(),
        trace_context: delivery.trace_context().cloned(),
        reason: reason.to_owned(),
        attempt: delivery.attempt(),
        pending: delivery.pending(),
        source_sequence: delivery.source_sequence(),
        consumer_sequence: delivery.consumer_sequence(),
        source_stream: source_stream.as_str().to_owned(),
        source_consumer: source_consumer.to_owned(),
    }
}

fn raw_quarantine_record(
    message: &jetstream::Message,
    info: DeliveryInfo,
    source_stream: &StreamName,
    source_consumer: &str,
    reason: &str,
) -> QuarantineRecord {
    let headers = message.headers.as_ref();
    let message_id = headers
        .and_then(|headers| single_header(headers, "Nats-Msg-Id").ok().flatten())
        .and_then(|value| MessageId::new(value).ok())
        .map_or_else(
            || "missing-or-invalid".to_owned(),
            |value| value.as_str().to_owned(),
        );
    let metadata = headers
        .and_then(|headers| caller_metadata(headers).ok())
        .unwrap_or_default();
    let trace_context = headers.and_then(|headers| trace_context(headers).ok().flatten());
    let payload = message.payload.as_ref();
    QuarantineRecord {
        message_id,
        address: message.subject.to_string(),
        payload_base64: BASE64.encode(payload),
        payload_size: Some(payload.len()),
        payload_sha256: Some(sha256_hex(payload)),
        payload_truncated: false,
        metadata,
        trace_context,
        reason: reason.to_owned(),
        attempt: info.attempt(),
        pending: info.pending(),
        source_sequence: info.source_sequence(),
        consumer_sequence: info.consumer_sequence(),
        source_stream: source_stream.as_str().to_owned(),
        source_consumer: source_consumer.to_owned(),
    }
}

async fn quarantine_or_nak(
    context: &jetstream::Context,
    quarantine_stream: &StreamName,
    publish_timeout: Duration,
    source: &jetstream::Message,
    record: QuarantineRecord,
    attempt: u32,
    maximum_delivery_attempts: u32,
) -> Result<(), ConsumeError> {
    if publish_quarantine(context, quarantine_stream, publish_timeout, &record)
        .await
        .is_err()
    {
        if quarantine_retry_available(attempt, maximum_delivery_attempts) {
            return apply_ack(source, AckKind::Nak(Some(DEFAULT_QUARANTINE_RETRY_DELAY))).await;
        }
        return Err(ConsumeError::new(ConsumeErrorKind::Quarantine));
    }
    apply_ack(source, AckKind::Term).await
}

const fn quarantine_retry_available(attempt: u32, maximum_delivery_attempts: u32) -> bool {
    attempt < maximum_delivery_attempts
}

async fn publish_quarantine(
    context: &jetstream::Context,
    quarantine_stream: &StreamName,
    publish_timeout: Duration,
    record: &QuarantineRecord,
) -> Result<(), NatsError> {
    let payload = encode_quarantine_record(record)?;
    let message_id = quarantine_message_id(record);
    let subject = quarantine_subject(&record.address)?;
    let headers = safe_headers(&CallerMetadata::new(), record.trace_context.as_ref());
    publish_confirmed(
        context,
        &subject,
        &payload,
        &message_id,
        quarantine_stream,
        headers,
        publish_timeout,
    )
    .await
    .map(|_| ())
}

fn encode_quarantine_record(record: &QuarantineRecord) -> Result<Vec<u8>, NatsError> {
    let payload = serde_json::to_vec(record).map_err(|_| NatsError::Serialization)?;
    if payload.len() <= MAX_QUARANTINE_RECORD_BYTES {
        return Ok(payload);
    }

    let mut fallback = record.clone();
    fallback
        .payload_base64
        .truncate(MAX_QUARANTINE_PAYLOAD_PREFIX_BASE64_BYTES);
    fallback.payload_truncated = true;
    let payload = serde_json::to_vec(&fallback).map_err(|_| NatsError::Serialization)?;
    if payload.len() > MAX_QUARANTINE_RECORD_BYTES {
        return Err(NatsError::PayloadTooLarge {
            actual: payload.len(),
            maximum: MAX_QUARANTINE_RECORD_BYTES,
        });
    }
    Ok(payload)
}

fn quarantine_subject(address: &str) -> Result<String, NatsError> {
    let address =
        MessageAddress::parse(address.to_owned()).map_err(|_| NatsError::InvalidMessage)?;
    let (_, routed) = address
        .as_str()
        .split_once('.')
        .ok_or(NatsError::InvalidMessage)?;
    Ok(format!("{}.quarantine.{routed}", address.application()))
}

fn quarantine_message_id(record: &QuarantineRecord) -> String {
    let mut hash = Sha256::new();
    hash.update(record.source_stream.as_bytes());
    hash.update([0]);
    hash.update(record.source_consumer.as_bytes());
    hash.update([0]);
    hash.update(record.source_sequence.to_be_bytes());
    let mut id = String::with_capacity(75);
    id.push_str("quarantine-");
    for byte in hash.finalize() {
        write!(&mut id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    id
}

fn sha256_hex(payload: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(payload) {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

async fn apply_ack(message: &jetstream::Message, kind: AckKind) -> Result<(), ConsumeError> {
    message
        .double_ack_with(kind)
        .await
        .map_err(|_| ConsumeError::new(ConsumeErrorKind::Disposition))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(payload: &[u8]) -> QuarantineRecord {
        QuarantineRecord {
            message_id: "message-1".to_owned(),
            address: "fast-inbox.command.commercial-access.evaluate".to_owned(),
            payload_base64: BASE64.encode(payload),
            payload_size: Some(payload.len()),
            payload_sha256: Some(sha256_hex(payload)),
            payload_truncated: false,
            metadata: CallerMetadata::new(),
            trace_context: None,
            reason: "invalid source message".to_owned(),
            attempt: 1,
            pending: 0,
            source_sequence: 42,
            consumer_sequence: 7,
            source_stream: "FAST_INBOX_COMMANDS".to_owned(),
            source_consumer: "fast-inbox--commercial-access--evaluate--v1".to_owned(),
        }
    }

    #[test]
    fn quarantine_subject_stays_inside_the_application_namespace() {
        assert_eq!(
            quarantine_subject("fast-inbox.command.commercial-access.evaluate").unwrap(),
            "fast-inbox.quarantine.command.commercial-access.evaluate"
        );
        assert!(quarantine_subject("invalid").is_err());
    }

    #[test]
    fn quarantine_publication_retries_are_bounded() {
        assert!(quarantine_retry_available(1, 3));
        assert!(quarantine_retry_available(2, 3));
        assert!(!quarantine_retry_available(3, 3));
        assert!(!quarantine_retry_available(4, 3));
    }

    #[test]
    fn quarantine_record_preserves_a_payload_that_fits() {
        let original = b"small payload";

        let encoded = encode_quarantine_record(&record(original)).unwrap();
        let decoded: QuarantineRecord = serde_json::from_slice(&encoded).unwrap();

        assert!(!decoded.payload_truncated());
        assert_eq!(decoded.payload_size(), Some(original.len()));
        assert_eq!(
            decoded.payload_sha256(),
            Some(sha256_hex(original).as_str())
        );
        assert_eq!(BASE64.decode(decoded.payload_base64()).unwrap(), original);
    }

    #[test]
    fn oversized_quarantine_record_uses_a_bounded_payload_prefix() {
        let original = vec![0x5a; MAX_QUARANTINE_RECORD_BYTES];

        let encoded = encode_quarantine_record(&record(&original)).unwrap();
        let decoded: QuarantineRecord = serde_json::from_slice(&encoded).unwrap();
        let prefix = BASE64.decode(decoded.payload_base64()).unwrap();

        assert!(encoded.len() <= MAX_QUARANTINE_RECORD_BYTES);
        assert!(decoded.payload_truncated());
        assert_eq!(decoded.payload_size(), Some(original.len()));
        assert_eq!(
            decoded.payload_sha256(),
            Some(sha256_hex(&original).as_str())
        );
        assert!(prefix.len() <= MAX_QUARANTINE_PAYLOAD_PREFIX_BYTES);
        assert_eq!(prefix, original[..prefix.len()]);
    }

    #[test]
    fn existing_quarantine_records_remain_readable() {
        let legacy = serde_json::json!({
            "message_id": "message-1",
            "address": "fast-inbox.command.commercial-access.evaluate",
            "payload_base64": "cGF5bG9hZA==",
            "metadata": {},
            "trace_context": null,
            "reason": "invalid source message",
            "attempt": 1,
            "pending": 0,
            "source_sequence": 42,
            "consumer_sequence": 7,
            "source_stream": "FAST_INBOX_COMMANDS",
            "source_consumer": "fast-inbox--commercial-access--evaluate--v1"
        });

        let record: QuarantineRecord = serde_json::from_value(legacy).unwrap();

        assert_eq!(record.payload_size(), None);
        assert_eq!(record.payload_sha256(), None);
        assert!(!record.payload_truncated());
    }
}
