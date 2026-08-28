use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    AddressKind, CallerMetadata, ConsumeError, ContractError, ContractErrorKind,
    MAX_MESSAGE_PAYLOAD_BYTES, MessageBuildError, MessageId, PublishableAddress, TraceContext,
    scope::validate_scope_segment,
};

pub const CONSUMER_NAME_CONVENTION: &str = "<application>--<context>--<purpose>--v<major>";
pub const DURABLE_NAME_CONVENTION: &str = "<application>--<context>--<purpose>--v<major>";
pub const MAX_CONSUMER_NAME_BYTES: usize = 256;
pub const MAX_CONCURRENCY: usize = 1024;
pub const MAX_DELIVERY_ATTEMPTS: u32 = 1000;
pub const MAX_PROCESSING_TIMEOUT: Duration = Duration::from_hours(24);
pub const MAX_RETRY_DELAY: Duration = Duration::from_hours(24);
pub const MAX_QUARANTINE_REASON_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ConsumerName(String);

impl ConsumerName {
    pub fn new(
        application: &str,
        context: &str,
        purpose: &str,
        major_version: u32,
    ) -> Result<Self, ContractError> {
        build_delivery_name(
            application,
            context,
            purpose,
            major_version,
            "consumer name",
        )
        .map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_delivery_name(value.into(), "consumer name").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn application(&self) -> &str {
        delivery_name_segment(&self.0, 0)
    }

    pub fn context(&self) -> &str {
        delivery_name_segment(&self.0, 1)
    }
}

impl fmt::Display for ConsumerName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ConsumerName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConsumerName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DurableName(String);

impl DurableName {
    pub fn new(
        application: &str,
        context: &str,
        purpose: &str,
        major_version: u32,
    ) -> Result<Self, ContractError> {
        build_delivery_name(application, context, purpose, major_version, "durable name").map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_delivery_name(value.into(), "durable name").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn application(&self) -> &str {
        delivery_name_segment(&self.0, 0)
    }

    pub fn context(&self) -> &str {
        delivery_name_segment(&self.0, 1)
    }
}

impl fmt::Display for DurableName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for DurableName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DurableName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerConfig<A>
where
    A: PublishableAddress,
{
    name: ConsumerName,
    durable_name: DurableName,
    address: A,
    ack_wait: Duration,
    processing_timeout: Duration,
    concurrency: usize,
    maximum_delivery_attempts: u32,
}

impl<A> ConsumerConfig<A>
where
    A: PublishableAddress,
{
    pub fn new(
        name: ConsumerName,
        durable_name: DurableName,
        address: A,
        ack_wait: Duration,
        processing_timeout: Duration,
        concurrency: usize,
        maximum_delivery_attempts: u32,
    ) -> Result<Self, ContractError> {
        if name.application() != address.application()
            || durable_name.application() != address.application()
        {
            return Err(ContractError::new(
                ContractErrorKind::InvalidFormat,
                "consumer application scope",
            ));
        }
        if name.context() != durable_name.context()
            || (address.kind() == AddressKind::Command && name.context() != address.context())
        {
            return Err(ContractError::new(
                ContractErrorKind::InvalidFormat,
                "consumer bounded-context scope",
            ));
        }
        if processing_timeout.is_zero() || processing_timeout > MAX_PROCESSING_TIMEOUT {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "processing timeout",
            ));
        }
        if ack_wait <= processing_timeout || ack_wait > MAX_PROCESSING_TIMEOUT {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "ack wait",
            ));
        }
        if concurrency == 0 || concurrency > MAX_CONCURRENCY {
            return Err(ContractError::bounded(
                ContractErrorKind::OutOfRange,
                "consumer concurrency",
                concurrency,
                MAX_CONCURRENCY,
            ));
        }
        if maximum_delivery_attempts == 0 || maximum_delivery_attempts > MAX_DELIVERY_ATTEMPTS {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "maximum delivery attempts",
            ));
        }
        Ok(Self {
            name,
            durable_name,
            address,
            ack_wait,
            processing_timeout,
            concurrency,
            maximum_delivery_attempts,
        })
    }

    pub const fn name(&self) -> &ConsumerName {
        &self.name
    }

    pub const fn durable_name(&self) -> &DurableName {
        &self.durable_name
    }

    pub const fn address(&self) -> &A {
        &self.address
    }

    pub const fn ack_wait(&self) -> Duration {
        self.ack_wait
    }

    pub const fn processing_timeout(&self) -> Duration {
        self.processing_timeout
    }

    pub const fn concurrency(&self) -> usize {
        self.concurrency
    }

    pub const fn maximum_delivery_attempts(&self) -> u32 {
        self.maximum_delivery_attempts
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryInfo {
    attempt: u32,
    pending: u64,
    source_sequence: u64,
    consumer_sequence: u64,
}

impl DeliveryInfo {
    pub fn new(
        attempt: u32,
        pending: u64,
        source_sequence: u64,
        consumer_sequence: u64,
    ) -> Result<Self, ContractError> {
        if attempt == 0 {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "delivery attempt",
            ));
        }
        if source_sequence == 0 || consumer_sequence == 0 {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "delivery sequence",
            ));
        }
        Ok(Self {
            attempt,
            pending,
            source_sequence,
            consumer_sequence,
        })
    }

    pub const fn attempt(self) -> u32 {
        self.attempt
    }

    pub const fn pending(self) -> u64 {
        self.pending
    }

    pub const fn source_sequence(self) -> u64 {
        self.source_sequence
    }

    pub const fn consumer_sequence(self) -> u64 {
        self.consumer_sequence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageDelivery<A>
where
    A: PublishableAddress,
{
    address: A,
    message_id: MessageId,
    payload: Vec<u8>,
    metadata: CallerMetadata,
    trace_context: Option<TraceContext>,
    info: DeliveryInfo,
}

impl<A> MessageDelivery<A>
where
    A: PublishableAddress,
{
    pub fn new(
        address: A,
        message_id: MessageId,
        payload: Vec<u8>,
        metadata: CallerMetadata,
        info: DeliveryInfo,
    ) -> Result<Self, MessageBuildError> {
        Self::new_with_trace_context(address, message_id, payload, metadata, None, info)
    }

    pub fn new_with_trace_context(
        address: A,
        message_id: MessageId,
        payload: Vec<u8>,
        metadata: CallerMetadata,
        trace_context: Option<TraceContext>,
        info: DeliveryInfo,
    ) -> Result<Self, MessageBuildError> {
        if payload.len() > MAX_MESSAGE_PAYLOAD_BYTES {
            return Err(MessageBuildError::payload_too_large(
                payload.len(),
                MAX_MESSAGE_PAYLOAD_BYTES,
            ));
        }
        Ok(Self {
            address,
            message_id,
            payload,
            metadata,
            trace_context,
            info,
        })
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

    pub const fn info(&self) -> DeliveryInfo {
        self.info
    }

    pub const fn attempt(&self) -> u32 {
        self.info.attempt
    }

    pub const fn pending(&self) -> u64 {
        self.info.pending
    }

    pub const fn source_sequence(&self) -> u64 {
        self.info.source_sequence
    }

    pub const fn consumer_sequence(&self) -> u64 {
        self.info.consumer_sequence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryDelay(Duration);

impl RetryDelay {
    pub fn new(value: Duration) -> Result<Self, ContractError> {
        if value.is_zero() || value > MAX_RETRY_DELAY {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "retry delay",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> Duration {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantineReason(String);

impl QuarantineReason {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ContractError::new(
                ContractErrorKind::Empty,
                "quarantine reason",
            ));
        }
        if value.len() > MAX_QUARANTINE_REASON_BYTES {
            return Err(ContractError::bounded(
                ContractErrorKind::TooLong,
                "quarantine reason",
                value.len(),
                MAX_QUARANTINE_REASON_BYTES,
            ));
        }
        if value.chars().any(char::is_control) {
            return Err(ContractError::new(
                ContractErrorKind::ControlCharacter,
                "quarantine reason",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    Acknowledge,
    RetryAfter(RetryDelay),
    Quarantine(QuarantineReason),
    Terminate,
}

#[async_trait]
pub trait MessageHandler<A>: Send + Sync
where
    A: PublishableAddress,
{
    async fn handle(&self, delivery: MessageDelivery<A>) -> DeliveryDisposition;
}

#[async_trait]
pub trait MessageConsumer<A>: Send + Sync
where
    A: PublishableAddress,
{
    async fn run(&self, handler: Arc<dyn MessageHandler<A>>) -> Result<(), ConsumeError>;
}

pub trait MessageConsumerFactory<A>: Send + Sync
where
    A: PublishableAddress,
{
    fn create(
        &self,
        config: ConsumerConfig<A>,
    ) -> Result<Arc<dyn MessageConsumer<A>>, ConsumeError>;
}

fn build_delivery_name(
    application: &str,
    context: &str,
    purpose: &str,
    major_version: u32,
    field: &'static str,
) -> Result<String, ContractError> {
    validate_scope_segment(application, field)?;
    validate_scope_segment(context, field)?;
    validate_scope_segment(purpose, field)?;
    let value = format!("{application}--{context}--{purpose}--v{major_version}");
    if value.len() > MAX_CONSUMER_NAME_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            field,
            value.len(),
            MAX_CONSUMER_NAME_BYTES,
        ));
    }
    Ok(value)
}

fn parse_delivery_name(value: String, field: &'static str) -> Result<String, ContractError> {
    if value.is_empty() {
        return Err(ContractError::new(ContractErrorKind::Empty, field));
    }
    if value.len() > MAX_CONSUMER_NAME_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            field,
            value.len(),
            MAX_CONSUMER_NAME_BYTES,
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            field,
        ));
    }
    let segments = value.split("--").collect::<Vec<_>>();
    if segments.len() != 4 {
        return Err(ContractError::new(ContractErrorKind::InvalidFormat, field));
    }
    let Some(version) = segments[3].strip_prefix('v') else {
        return Err(ContractError::new(ContractErrorKind::InvalidFormat, field));
    };
    let major_version = version
        .parse::<u32>()
        .map_err(|_| ContractError::new(ContractErrorKind::InvalidFormat, field))?;
    if version != major_version.to_string() {
        return Err(ContractError::new(ContractErrorKind::InvalidFormat, field));
    }
    build_delivery_name(segments[0], segments[1], segments[2], major_version, field)?;
    Ok(value)
}

fn delivery_name_segment(value: &str, index: usize) -> &str {
    value.split("--").nth(index).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandAddress, MessageBuildErrorKind};

    #[test]
    fn consumer_and_durable_names_are_stable_and_parseable() {
        let consumer = ConsumerName::new("acme", "orders", "fulfillment", 1).unwrap();
        let durable = DurableName::parse("acme--orders--fulfillment--v1").unwrap();
        assert_eq!(consumer.as_str(), "acme--orders--fulfillment--v1");
        assert_eq!(durable.as_str(), consumer.as_str());
        assert_eq!(consumer.application(), "acme");
        assert_eq!(consumer.context(), "orders");
        assert_eq!(durable.application(), "acme");
        assert_eq!(durable.context(), "orders");
        assert!(ConsumerName::parse("acme--orders--fulfillment--v01").is_err());
        assert!(DurableName::new("Acme", "orders", "fulfillment", 1).is_err());
    }

    #[test]
    fn consumer_config_bounds_runtime_and_retry_policy() {
        let address = CommandAddress::new("acme", "orders", "place-order").unwrap();
        let consumer = ConsumerName::new("acme", "orders", "fulfillment", 1).unwrap();
        let durable = DurableName::new("acme", "orders", "fulfillment", 1).unwrap();
        assert!(
            ConsumerConfig::new(
                consumer.clone(),
                durable.clone(),
                address.clone(),
                Duration::from_secs(30),
                Duration::ZERO,
                1,
                5,
            )
            .is_err()
        );
        for ack_wait in [
            Duration::ZERO,
            Duration::from_secs(29),
            Duration::from_secs(30),
        ] {
            assert!(
                ConsumerConfig::new(
                    consumer.clone(),
                    durable.clone(),
                    address.clone(),
                    ack_wait,
                    Duration::from_secs(30),
                    1,
                    5,
                )
                .is_err()
            );
        }
        let config = ConsumerConfig::new(
            consumer.clone(),
            durable.clone(),
            address.clone(),
            Duration::from_secs(45),
            Duration::from_secs(30),
            1,
            5,
        )
        .unwrap();
        assert_eq!(config.ack_wait(), Duration::from_secs(45));
        assert_eq!(config.processing_timeout(), Duration::from_secs(30));
        assert!(
            ConsumerConfig::new(
                consumer,
                durable,
                address,
                Duration::from_secs(45),
                Duration::from_secs(30),
                1,
                0,
            )
            .is_err()
        );
    }

    #[test]
    fn consumer_config_rejects_cross_application_names() {
        let address = CommandAddress::new("acme", "orders", "place-order").unwrap();

        let error = ConsumerConfig::new(
            ConsumerName::new("other", "orders", "fulfillment", 1).unwrap(),
            DurableName::new("acme", "orders", "fulfillment", 1).unwrap(),
            address,
            Duration::from_secs(45),
            Duration::from_secs(30),
            1,
            5,
        )
        .unwrap_err();

        assert_eq!(error.kind(), ContractErrorKind::InvalidFormat);
        assert_eq!(error.field(), "consumer application scope");
    }

    #[test]
    fn command_consumers_stay_in_context_but_integration_consumers_can_cross_contexts() {
        let command = CommandAddress::new("acme", "orders", "place-order").unwrap();
        let name = ConsumerName::new("acme", "fulfillment", "orders", 1).unwrap();
        let durable = DurableName::new("acme", "fulfillment", "orders", 1).unwrap();

        let error = ConsumerConfig::new(
            name.clone(),
            durable.clone(),
            command,
            Duration::from_secs(45),
            Duration::from_secs(30),
            1,
            5,
        )
        .unwrap_err();
        assert_eq!(error.field(), "consumer bounded-context scope");

        let integration_event =
            crate::IntegrationEventAddress::new("acme", "orders", "order-placed").unwrap();
        assert!(
            ConsumerConfig::new(
                name,
                durable,
                integration_event,
                Duration::from_secs(45),
                Duration::from_secs(30),
                1,
                5,
            )
            .is_ok()
        );
    }

    #[test]
    fn deliveries_expose_identity_metadata_and_delivery_progress() {
        let info = DeliveryInfo::new(2, 10, 42, 7).unwrap();
        let trace_context =
            TraceContext::new("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").unwrap();
        let delivery = MessageDelivery::new_with_trace_context(
            CommandAddress::new("acme", "orders", "place-order").unwrap(),
            MessageId::new("message-1").unwrap(),
            b"{}".to_vec(),
            CallerMetadata::new(),
            Some(trace_context),
            info,
        )
        .unwrap();
        assert_eq!(delivery.message_id().as_str(), "message-1");
        assert_eq!(delivery.attempt(), 2);
        assert_eq!(delivery.pending(), 10);
        assert_eq!(delivery.source_sequence(), 42);
        assert_eq!(delivery.consumer_sequence(), 7);
        assert!(delivery.trace_context().is_some());

        let error = MessageDelivery::new(
            CommandAddress::new("acme", "orders", "place-order").unwrap(),
            MessageId::new("message-2").unwrap(),
            vec![0; MAX_MESSAGE_PAYLOAD_BYTES + 1],
            CallerMetadata::new(),
            info,
        )
        .unwrap_err();
        assert_eq!(error.kind(), MessageBuildErrorKind::PayloadTooLarge);
    }

    #[test]
    fn retry_and_quarantine_dispositions_are_bounded() {
        let retry = RetryDelay::new(Duration::from_secs(5)).unwrap();
        assert_eq!(
            DeliveryDisposition::RetryAfter(retry),
            DeliveryDisposition::RetryAfter(retry)
        );
        assert!(RetryDelay::new(Duration::ZERO).is_err());
        assert!(QuarantineReason::new("invalid payload").is_ok());
        assert!(QuarantineReason::new("line\nbreak").is_err());
    }
}
