use std::{sync::Arc, time::Duration};

use async_nats::jetstream::{
    self,
    consumer::{self, AckPolicy, DeliverPolicy},
    message::AckKind,
    stream::{RawMessageError, RawMessageErrorKind},
};
use futures_util::TryStreamExt;
use rostfrei_core::{
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandlerError,
    DomainEventHandlerErrorKind, EventStore, EventStoreErrorKind, MAX_EVENTS_PER_BATCH,
};
use rostfrei_messaging_core::{ConsumerName, DurableName, MAX_PROCESSING_TIMEOUT, RetryDelay};
use thiserror::Error;
use tokio::{sync::watch, time::timeout};

use crate::{
    event_store::{DecodedEvent, NatsEventStore, decode_consumed_event},
    event_store_config::NatsEventStoreConfig,
};

const PULL_EXPIRATION: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DomainEventConsumerErrorKind {
    InvalidConfiguration,
    Unavailable,
    InvalidCommittedEvent,
    PermanentlyUnsupported,
    OperatorBlocked,
    Acknowledgement,
    Ended,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{kind:?}: {message}")]
pub struct DomainEventConsumerError {
    kind: DomainEventConsumerErrorKind,
    message: String,
}

impl DomainEventConsumerError {
    fn new(kind: DomainEventConsumerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> DomainEventConsumerErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsDomainEventConsumerConfig {
    name: ConsumerName,
    durable_name: DurableName,
    ack_wait: Duration,
    processing_timeout: Duration,
    retry_delay: RetryDelay,
}

impl NatsDomainEventConsumerConfig {
    pub fn new(
        name: ConsumerName,
        durable_name: DurableName,
        ack_wait: Duration,
        processing_timeout: Duration,
        retry_delay: RetryDelay,
    ) -> Result<Self, DomainEventConsumerError> {
        if name.application() != durable_name.application()
            || name.context() != durable_name.context()
        {
            return Err(invalid_configuration(
                "consumer and durable names must have the same application and bounded context",
            ));
        }
        if ack_wait.is_zero()
            || ack_wait > MAX_PROCESSING_TIMEOUT
            || processing_timeout.is_zero()
            || processing_timeout >= ack_wait
        {
            return Err(invalid_configuration(
                "ACK wait must exceed the non-zero processing timeout and stay within 24 hours",
            ));
        }
        Ok(Self {
            name,
            durable_name,
            ack_wait,
            processing_timeout,
            retry_delay,
        })
    }

    pub const fn name(&self) -> &ConsumerName {
        &self.name
    }

    pub const fn durable_name(&self) -> &DurableName {
        &self.durable_name
    }

    pub const fn ack_wait(&self) -> Duration {
        self.ack_wait
    }

    pub const fn processing_timeout(&self) -> Duration {
        self.processing_timeout
    }

    pub const fn retry_delay(&self) -> RetryDelay {
        self.retry_delay
    }
}

pub struct NatsDomainEventConsumer {
    context: jetstream::Context,
    event_store: NatsEventStore,
    config: NatsDomainEventConsumerConfig,
    dispatcher: Arc<DomainEventDispatcher>,
}

impl NatsDomainEventConsumer {
    pub async fn connect(
        context: jetstream::Context,
        event_store: NatsEventStoreConfig,
        config: NatsDomainEventConsumerConfig,
        dispatcher: Arc<DomainEventDispatcher>,
    ) -> Result<Self, DomainEventConsumerError> {
        let event_store = NatsEventStore::connect(context.clone(), event_store)
            .await
            .map_err(|error| {
                let kind = match error.kind() {
                    EventStoreErrorKind::ConfigurationMismatch
                    | EventStoreErrorKind::InvalidRequest => {
                        DomainEventConsumerErrorKind::InvalidConfiguration
                    }
                    _ => DomainEventConsumerErrorKind::Unavailable,
                };
                DomainEventConsumerError::new(kind, error.to_string())
            })?;
        let stream = context
            .get_stream(event_store.config().stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        let consumer: consumer::PullConsumer = stream
            .get_consumer(config.durable_name().as_str())
            .await
            .map_err(|error| unavailable(format!("failed to get durable consumer: {error}")))?;
        verify_consumer(&consumer, event_store.config(), &config)?;
        Ok(Self {
            context,
            event_store,
            config,
            dispatcher,
        })
    }

    #[allow(clippy::too_many_lines)]
    pub async fn run_until_shutdown(
        self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), DomainEventConsumerError> {
        if *shutdown.borrow() {
            return Ok(());
        }
        let stream = self
            .context
            .get_stream(self.event_store.config().stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        let consumer: consumer::PullConsumer = stream
            .get_consumer(self.config.durable_name().as_str())
            .await
            .map_err(|error| unavailable(format!("failed to get durable consumer: {error}")))?;
        verify_consumer(&consumer, self.event_store.config(), &self.config)?;
        loop {
            let mut first_delivery = consumer
                .batch()
                .max_messages(1)
                .expires(PULL_EXPIRATION)
                .messages()
                .await
                .map_err(|error| {
                    unavailable(format!("failed to start durable delivery: {error}"))
                })?;
            let first = tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                    continue;
                }
                message = first_delivery.try_next() => {
                    let delivery = message.map_err(|error| {
                        unavailable(format!("domain-event delivery failed: {error}"))
                    })?;
                    let Some(message) = delivery else {
                        continue;
                    };
                    message
                },
            };
            let first = self.buffer_delivery(first)?;
            let consumer_info = consumer.get_info().await.map_err(|error| {
                unavailable(format!("failed to inspect durable progress: {error}"))
            })?;
            let search_start = consumer_info
                .ack_floor
                .stream_sequence
                .checked_add(1)
                .ok_or_else(|| invalid_committed_event("durable sequence space overflowed"))?;
            let expected_sequence = stream
                .get_first_raw_message_by_subject(
                    &self.event_store.config().aggregate_subject_filter(),
                    search_start,
                )
                .await
                .map_err(|error| earliest_unresolved_lookup_error(&error))?
                .sequence;
            if first.event.stream_sequence != expected_sequence {
                tracing::warn!(
                    durable = %self.config.durable_name(),
                    expected_sequence,
                    delivered_sequence = first.event.stream_sequence,
                    "deferring delivery until the durable's earliest unresolved event is available"
                );
                defer_delivery(&first.delivery, self.config.ack_wait()).await?;
                if !wait_for_retry(self.config.ack_wait(), &mut shutdown).await {
                    return Ok(());
                }
                continue;
            }

            let mut commit = self
                .reconstruct_acknowledged_prefix(&stream, &first.event)
                .await?;
            if commit.is_empty() {
                validate_first_event(&first.event)?;
            } else {
                validate_next_event(&commit, &first.event)?;
            }
            let event_count = first.event.decoded.transaction_event_count;
            let commit_id = first.event.decoded.commit_id.clone();
            let mut live_deliveries = LiveDeliveries::new(first.delivery);
            commit.push(first.event);
            let remaining = event_count.checked_sub(commit.len()).ok_or_else(|| {
                invalid_committed_event("committed transaction exceeds its declared event count")
            })?;
            if remaining > 0 {
                let mut remaining_deliveries = consumer
                    .batch()
                    .max_messages(remaining)
                    .expires(PULL_EXPIRATION)
                    .messages()
                    .await
                    .map_err(|error| {
                        unavailable(format!("failed to continue durable delivery: {error}"))
                    })?;
                while commit.len() < event_count {
                    let message = tokio::select! {
                        changed = shutdown.changed() => {
                            if changed.is_err() || *shutdown.borrow() {
                                return Ok(());
                            }
                            continue;
                        }
                        message = remaining_deliveries.try_next() => message
                            .map_err(|error| unavailable(format!("domain-event delivery failed: {error}")))?
                            .ok_or_else(|| unavailable(
                                "domain-event delivery ended inside a committed event batch",
                            ))?,
                    };
                    let next = self.buffer_delivery(message)?;
                    validate_next_event(&commit, &next.event)?;
                    live_deliveries.push(next.delivery);
                    commit.push(next.event);
                }
            }
            validate_complete_transaction(&commit)?;
            self.validate_transaction_receipt(&commit).await?;

            match timeout(
                self.config.processing_timeout(),
                self.handle_commit(&commit),
            )
            .await
            {
                Ok(Ok(())) => acknowledge_commit(live_deliveries.last()).await?,
                Ok(Err(error)) if error.kind() == DomainEventHandlerErrorKind::Retryable => {
                    tracing::warn!(
                        durable = %self.config.durable_name(),
                        commit_id = %commit_id,
                        error = %error,
                        "domain-event handler requested redelivery"
                    );
                    let delay = self.config.retry_delay().get();
                    retry_commit(&live_deliveries, delay).await?;
                    if !wait_for_retry(delay, &mut shutdown).await {
                        return Ok(());
                    }
                }
                Ok(Err(error)) => {
                    tracing::error!(
                        durable = %self.config.durable_name(),
                        commit_id = %commit_id,
                        error = %error,
                        "domain-event durable is blocked and requires operator action"
                    );
                    return Err(handler_blocked(&error));
                }
                Err(_) => {
                    tracing::warn!(
                        durable = %self.config.durable_name(),
                        commit_id = %commit_id,
                        "domain-event handler timed out and will be redelivered"
                    );
                    let delay = self.config.retry_delay().get();
                    retry_commit(&live_deliveries, delay).await?;
                    if !wait_for_retry(delay, &mut shutdown).await {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn buffer_delivery(
        &self,
        message: jetstream::Message,
    ) -> Result<LiveDomainEvent, DomainEventConsumerError> {
        let info = message
            .info()
            .map_err(|error| unavailable(format!("delivery metadata is unavailable: {error}")))?;
        if info.stream != self.event_store.config().stream_name()
            || info.consumer != self.config.durable_name().as_str()
            || info.stream_sequence == 0
            || info.consumer_sequence == 0
        {
            return Err(invalid_committed_event(
                "delivery metadata does not identify the configured durable consumer",
            ));
        }
        let stream_sequence = info.stream_sequence;
        let headers = message
            .headers
            .as_ref()
            .ok_or_else(|| invalid_committed_event("stored domain event has no headers"))?;
        let decoded = decode_consumed_event(
            self.event_store.config(),
            message.subject.as_str(),
            headers,
            &message.payload,
        )
        .map_err(|error| invalid_committed_event(error.to_string()))?;
        Ok(LiveDomainEvent {
            event: BufferedDomainEvent {
                stream_sequence,
                decoded,
            },
            delivery: message,
        })
    }

    async fn reconstruct_acknowledged_prefix(
        &self,
        stream: &jetstream::stream::Stream,
        first: &BufferedDomainEvent,
    ) -> Result<Vec<BufferedDomainEvent>, DomainEventConsumerError> {
        if first.decoded.transaction_event_ordinal == 0 {
            return Ok(Vec::new());
        }
        let transaction_event_ordinal = u64::try_from(first.decoded.transaction_event_ordinal)
            .map_err(|_| {
                invalid_committed_event("transaction event ordinal cannot be represented")
            })?;
        let start_sequence = first
            .stream_sequence
            .checked_sub(transaction_event_ordinal)
            .ok_or_else(|| invalid_committed_event("commit start sequence underflowed"))?;
        let mut prefix = Vec::with_capacity(first.decoded.transaction_event_ordinal);
        for sequence in start_sequence..first.stream_sequence {
            let raw = stream
                .get_raw_message(sequence)
                .await
                .map_err(|error| acknowledged_prefix_lookup_error(&error))?;
            let decoded = decode_consumed_event(
                self.event_store.config(),
                raw.subject.as_str(),
                &raw.headers,
                &raw.payload,
            )
            .map_err(|error| invalid_committed_event(error.to_string()))?;
            let event = BufferedDomainEvent {
                stream_sequence: raw.sequence,
                decoded,
            };
            if prefix.is_empty() {
                validate_first_event(&event)?;
            } else {
                validate_next_event(&prefix, &event)?;
            }
            prefix.push(event);
        }
        Ok(prefix)
    }

    async fn validate_transaction_receipt(
        &self,
        commit: &[BufferedDomainEvent],
    ) -> Result<(), DomainEventConsumerError> {
        let first = commit
            .first()
            .ok_or_else(|| invalid_committed_event("committed transaction is empty"))?;
        if commit
            .iter()
            .any(|event| event.decoded.is_transactional != first.decoded.is_transactional)
        {
            return Err(invalid_committed_event(
                "committed event batch mixes transactional and direct event schemas",
            ));
        }
        if !first.decoded.is_transactional {
            return Ok(());
        }

        let receipt = self
            .event_store
            .load_transaction_receipt(
                first.decoded.recorded.stream_id(),
                &first.decoded.operation_id,
            )
            .await
            .map_err(|error| match error.kind() {
                EventStoreErrorKind::Unavailable => unavailable(format!(
                    "failed to load committed transaction receipt: {error}"
                )),
                _ => invalid_committed_event(format!(
                    "committed transaction receipt is invalid: {error}"
                )),
            })?
            .ok_or_else(|| {
                invalid_committed_event("committed transactional events have no durable receipt")
            })?;
        let receipt_events = receipt.events();
        if receipt_events.len() != commit.len()
            || receipt_events
                .iter()
                .zip(commit)
                .any(|(receipt_event, buffered)| receipt_event != &buffered.decoded.recorded)
        {
            return Err(invalid_committed_event(
                "committed transactional events do not match their durable receipt",
            ));
        }
        Ok(())
    }

    async fn handle_commit(
        &self,
        commit: &[BufferedDomainEvent],
    ) -> Result<(), DomainEventHandlerError> {
        for event in commit {
            match self.dispatcher.dispatch(&event.decoded.recorded).await? {
                DomainEventDispatchOutcome::Handled | DomainEventDispatchOutcome::Ignored => {}
            }
        }
        Ok(())
    }
}

struct BufferedDomainEvent {
    stream_sequence: u64,
    decoded: DecodedEvent,
}

struct LiveDomainEvent {
    event: BufferedDomainEvent,
    delivery: jetstream::Message,
}

struct LiveDeliveries {
    first: jetstream::Message,
    rest: Vec<jetstream::Message>,
}

impl LiveDeliveries {
    const fn new(first: jetstream::Message) -> Self {
        Self {
            first,
            rest: Vec::new(),
        }
    }

    fn push(&mut self, delivery: jetstream::Message) {
        self.rest.push(delivery);
    }

    fn last(&self) -> &jetstream::Message {
        self.rest.last().unwrap_or(&self.first)
    }

    fn iter(&self) -> impl Iterator<Item = &jetstream::Message> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

pub async fn provision_domain_event_consumer(
    context: &jetstream::Context,
    event_store: &NatsEventStoreConfig,
    config: &NatsDomainEventConsumerConfig,
) -> Result<consumer::Info, DomainEventConsumerError> {
    let consumer = context
        .create_consumer_on_stream(
            durable_domain_event_consumer_config(event_store, config)?,
            event_store.stream_name(),
        )
        .await
        .map_err(|error| unavailable(format!("failed to provision durable consumer: {error}")))?;
    Ok(consumer.cached_info().clone())
}

fn durable_domain_event_consumer_config(
    event_store: &NatsEventStoreConfig,
    config: &NatsDomainEventConsumerConfig,
) -> Result<consumer::pull::Config, DomainEventConsumerError> {
    if config.name().application() != event_store.application().as_str()
        || config.durable_name().application() != event_store.application().as_str()
        || config.name().context() != event_store.bounded_context().as_str()
        || config.durable_name().context() != event_store.bounded_context().as_str()
    {
        return Err(invalid_configuration(
            "domain-event consumer belongs to a different application or bounded context",
        ));
    }
    let max_pending = i64::try_from(MAX_EVENTS_PER_BATCH)
        .map_err(|_| invalid_configuration("maximum commit size cannot be represented"))?;
    Ok(consumer::pull::Config {
        durable_name: Some(config.durable_name().as_str().to_owned()),
        name: Some(config.durable_name().as_str().to_owned()),
        description: Some(config.name().as_str().to_owned()),
        deliver_policy: DeliverPolicy::All,
        ack_policy: AckPolicy::All,
        ack_wait: config.ack_wait(),
        max_deliver: -1,
        filter_subject: event_store.aggregate_subject_filter(),
        max_ack_pending: max_pending,
        max_batch: max_pending,
        max_waiting: 1,
        ..Default::default()
    })
}

fn verify_consumer(
    consumer: &consumer::PullConsumer,
    event_store: &NatsEventStoreConfig,
    config: &NatsDomainEventConsumerConfig,
) -> Result<(), DomainEventConsumerError> {
    let expected = durable_domain_event_consumer_config(event_store, config)?;
    let actual = &consumer.cached_info().config;
    if consumer.cached_info().name != config.durable_name().as_str()
        || actual.deliver_subject.is_some()
        || actual.deliver_group.is_some()
        || actual.name.as_deref() != expected.name.as_deref()
        || actual.durable_name.as_deref() != expected.durable_name.as_deref()
        || actual.description != expected.description
        || actual.deliver_policy != DeliverPolicy::All
        || actual.ack_policy != AckPolicy::All
        || actual.ack_wait != expected.ack_wait
        || actual.max_deliver != -1
        || actual.filter_subject != expected.filter_subject
        || actual.replay_policy != expected.replay_policy
        || actual.rate_limit != expected.rate_limit
        || actual.sample_frequency != expected.sample_frequency
        || actual.max_ack_pending != expected.max_ack_pending
        || actual.max_batch != expected.max_batch
        || actual.max_waiting != 1
        || actual.headers_only != expected.headers_only
        || actual.flow_control
        || !actual.idle_heartbeat.is_zero()
        || actual.max_bytes != expected.max_bytes
        || actual.max_expires != expected.max_expires
        || actual.inactive_threshold != expected.inactive_threshold
        || actual.num_replicas != expected.num_replicas
        || actual.memory_storage != expected.memory_storage
        || actual.backoff != expected.backoff
    {
        return Err(invalid_configuration(
            "existing durable consumer configuration does not match",
        ));
    }
    Ok(())
}

fn validate_next_event(
    commit: &[BufferedDomainEvent],
    next: &BufferedDomainEvent,
) -> Result<(), DomainEventConsumerError> {
    let first = &commit
        .first()
        .ok_or_else(|| invalid_committed_event("committed event batch is empty"))?
        .decoded;
    let previous = commit
        .last()
        .ok_or_else(|| invalid_committed_event("committed event batch is empty"))?;
    let same_stream = next.decoded.recorded.stream_id() == previous.decoded.recorded.stream_id();
    let stream_coordinates_valid = if same_stream {
        next.decoded.commit_id == previous.decoded.commit_id
            && previous.decoded.event_ordinal.checked_add(1) == Some(next.decoded.event_ordinal)
            && next.decoded.event_count == previous.decoded.event_count
            && previous
                .decoded
                .recorded
                .stream_version()
                .value()
                .checked_add(1)
                == Some(next.decoded.recorded.stream_version().value())
    } else {
        previous.decoded.event_ordinal.checked_add(1) == Some(previous.decoded.event_count)
            && next.decoded.event_ordinal == 0
            && !commit.iter().any(|event| {
                event.decoded.recorded.stream_id() == next.decoded.recorded.stream_id()
            })
    };
    if next.decoded.transaction_event_ordinal != commit.len()
        || next.decoded.transaction_event_count != first.transaction_event_count
        || next.decoded.batch_id != first.batch_id
        || next.decoded.operation_id != first.operation_id
        || next.decoded.operation_fingerprint != first.operation_fingerprint
        || next.decoded.recorded.correlation_id() != first.recorded.correlation_id()
        || next.decoded.recorded.causation_id() != first.recorded.causation_id()
        || !stream_coordinates_valid
        || previous.stream_sequence.checked_add(1) != Some(next.stream_sequence)
    {
        return Err(invalid_committed_event(
            "committed event batch is missing, reordered, or inconsistent",
        ));
    }
    Ok(())
}

fn validate_first_event(first: &BufferedDomainEvent) -> Result<(), DomainEventConsumerError> {
    if first.decoded.transaction_event_ordinal != 0 || first.decoded.event_ordinal != 0 {
        return Err(invalid_committed_event(
            "committed transaction does not start at the first event of a local batch",
        ));
    }
    Ok(())
}

fn validate_complete_transaction(
    commit: &[BufferedDomainEvent],
) -> Result<(), DomainEventConsumerError> {
    let first = commit
        .first()
        .ok_or_else(|| invalid_committed_event("committed transaction is empty"))?;
    let last = commit
        .last()
        .ok_or_else(|| invalid_committed_event("committed transaction is empty"))?;
    if commit.len() != first.decoded.transaction_event_count
        || last.decoded.transaction_event_ordinal.checked_add(1)
            != Some(first.decoded.transaction_event_count)
        || last.decoded.event_ordinal.checked_add(1) != Some(last.decoded.event_count)
    {
        return Err(invalid_committed_event(
            "committed transaction ends inside a local event batch",
        ));
    }
    Ok(())
}

async fn acknowledge_commit(delivery: &jetstream::Message) -> Result<(), DomainEventConsumerError> {
    delivery.double_ack().await.map_err(|error| {
        DomainEventConsumerError::new(
            DomainEventConsumerErrorKind::Acknowledgement,
            format!("failed to acknowledge committed event batch: {error}"),
        )
    })
}

async fn retry_commit(
    deliveries: &LiveDeliveries,
    delay: Duration,
) -> Result<(), DomainEventConsumerError> {
    for message in deliveries.iter() {
        message
            .double_ack_with(AckKind::Nak(Some(delay)))
            .await
            .map_err(|error| {
                DomainEventConsumerError::new(
                    DomainEventConsumerErrorKind::Acknowledgement,
                    format!("failed to NAK committed event batch: {error}"),
                )
            })?;
    }
    Ok(())
}

async fn defer_delivery(
    delivery: &jetstream::Message,
    delay: Duration,
) -> Result<(), DomainEventConsumerError> {
    delivery
        .double_ack_with(AckKind::Nak(Some(delay)))
        .await
        .map_err(|error| {
            DomainEventConsumerError::new(
                DomainEventConsumerErrorKind::Acknowledgement,
                format!("failed to defer out-of-order durable delivery: {error}"),
            )
        })
}

async fn wait_for_retry(delay: Duration, shutdown: &mut watch::Receiver<bool>) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            () = &mut sleep => return true,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return false;
                }
            }
        }
    }
}

fn handler_blocked(error: &DomainEventHandlerError) -> DomainEventConsumerError {
    let kind = match error.kind() {
        DomainEventHandlerErrorKind::Retryable => DomainEventConsumerErrorKind::Unavailable,
        DomainEventHandlerErrorKind::PermanentlyUnsupported => {
            DomainEventConsumerErrorKind::PermanentlyUnsupported
        }
        DomainEventHandlerErrorKind::InvalidCommittedEvent => {
            DomainEventConsumerErrorKind::InvalidCommittedEvent
        }
        DomainEventHandlerErrorKind::OperatorBlocking => {
            DomainEventConsumerErrorKind::OperatorBlocked
        }
        _ => DomainEventConsumerErrorKind::OperatorBlocked,
    };
    DomainEventConsumerError::new(kind, error.to_string())
}

fn invalid_configuration(message: impl Into<String>) -> DomainEventConsumerError {
    DomainEventConsumerError::new(DomainEventConsumerErrorKind::InvalidConfiguration, message)
}

fn unavailable(message: impl Into<String>) -> DomainEventConsumerError {
    DomainEventConsumerError::new(DomainEventConsumerErrorKind::Unavailable, message)
}

fn acknowledged_prefix_lookup_error(error: &RawMessageError) -> DomainEventConsumerError {
    raw_message_lookup_error(
        error,
        "acknowledged commit prefix",
        "reconstruct acknowledged commit prefix",
    )
}

fn earliest_unresolved_lookup_error(error: &RawMessageError) -> DomainEventConsumerError {
    raw_message_lookup_error(
        error,
        "the durable's earliest unresolved event",
        "locate the durable's earliest unresolved event",
    )
}

fn raw_message_lookup_error(
    error: &RawMessageError,
    missing_history: &str,
    lookup: &str,
) -> DomainEventConsumerError {
    if error.kind() == RawMessageErrorKind::NoMessageFound {
        invalid_committed_event(format!(
            "{missing_history} is missing from authoritative event history: {error}"
        ))
    } else {
        unavailable(format!("failed to {lookup}: {error}"))
    }
}

fn invalid_committed_event(message: impl Into<String>) -> DomainEventConsumerError {
    DomainEventConsumerError::new(DomainEventConsumerErrorKind::InvalidCommittedEvent, message)
}

#[cfg(test)]
mod tests {
    use rostfrei_core::{
        AggregateId, AggregateType, ContentFingerprint, ExecutionMetadata, OperationId,
        RecordedEvent, StreamId, StreamVersion,
    };
    use rostfrei_messaging_core::{ApplicationName, ConsumerName, DurableName};

    use super::*;

    #[test]
    fn domain_event_stream_and_consumer_share_the_same_subject_filter() {
        let context = ApplicationName::new("acme")
            .unwrap()
            .bounded_context("orders")
            .unwrap();
        let event_store = NatsEventStoreConfig::for_bounded_context(&context).unwrap();
        let consumer = NatsDomainEventConsumerConfig::new(
            ConsumerName::new("acme", "orders", "projection", 1).unwrap(),
            DurableName::new("acme", "orders", "projection", 1).unwrap(),
            Duration::from_secs(5),
            Duration::from_secs(2),
            RetryDelay::new(Duration::from_millis(100)).unwrap(),
        )
        .unwrap();

        let stream_subjects = event_store.stream_config().subjects;
        let consumer_subject = durable_domain_event_consumer_config(&event_store, &consumer)
            .unwrap()
            .filter_subject;

        assert!(stream_subjects.contains(&consumer_subject));
    }

    #[test]
    fn acknowledged_prefix_lookup_distinguishes_missing_history_from_unavailability() {
        let missing = acknowledged_prefix_lookup_error(&RawMessageError::new(
            RawMessageErrorKind::NoMessageFound,
        ));
        let unavailable =
            acknowledged_prefix_lookup_error(&RawMessageError::new(RawMessageErrorKind::Other));

        assert_eq!(
            missing.kind(),
            DomainEventConsumerErrorKind::InvalidCommittedEvent
        );
        assert_eq!(
            unavailable.kind(),
            DomainEventConsumerErrorKind::Unavailable
        );
    }

    #[test]
    fn earliest_unresolved_lookup_distinguishes_missing_history_from_unavailability() {
        let missing = earliest_unresolved_lookup_error(&RawMessageError::new(
            RawMessageErrorKind::NoMessageFound,
        ));
        let unavailable =
            earliest_unresolved_lookup_error(&RawMessageError::new(RawMessageErrorKind::Other));

        assert_eq!(
            missing.kind(),
            DomainEventConsumerErrorKind::InvalidCommittedEvent
        );
        assert_eq!(
            unavailable.kind(),
            DomainEventConsumerErrorKind::Unavailable
        );
    }

    #[test]
    fn transaction_grouping_rejects_early_stream_switch() {
        let source = stream_id("source");
        let destination = stream_id("destination");
        let first = buffered_event(&source, 1, 0, 2, 0, 2, 1);
        let next = buffered_event(&destination, 1, 0, 1, 1, 2, 2);

        let error = validate_next_event(&[first], &next).unwrap_err();

        assert_eq!(
            error.kind(),
            DomainEventConsumerErrorKind::InvalidCommittedEvent
        );
    }

    #[test]
    fn transaction_grouping_rejects_incomplete_final_local_commit() {
        let source = stream_id("source");
        let destination = stream_id("destination");
        let first = buffered_event(&source, 1, 0, 1, 0, 2, 1);
        let final_event = buffered_event(&destination, 1, 0, 2, 1, 2, 2);
        let mut commit = vec![first];
        validate_next_event(&commit, &final_event).unwrap();
        commit.push(final_event);

        let error = validate_complete_transaction(&commit).unwrap_err();

        assert_eq!(
            error.kind(),
            DomainEventConsumerErrorKind::InvalidCommittedEvent
        );
    }

    #[test]
    fn domain_event_consumer_rejects_cross_scope_names() {
        let context = ApplicationName::new("acme")
            .unwrap()
            .bounded_context("orders")
            .unwrap();
        let event_store = NatsEventStoreConfig::for_bounded_context(&context).unwrap();
        assert_eq!(
            NatsDomainEventConsumerConfig::new(
                ConsumerName::new("other", "orders", "projection", 1).unwrap(),
                DurableName::new("acme", "orders", "projection", 1).unwrap(),
                Duration::from_secs(5),
                Duration::from_secs(2),
                RetryDelay::new(Duration::from_millis(100)).unwrap(),
            )
            .unwrap_err()
            .kind(),
            DomainEventConsumerErrorKind::InvalidConfiguration
        );

        let consumer = NatsDomainEventConsumerConfig::new(
            ConsumerName::new("other", "orders", "projection", 1).unwrap(),
            DurableName::new("other", "orders", "projection", 1).unwrap(),
            Duration::from_secs(5),
            Duration::from_secs(2),
            RetryDelay::new(Duration::from_millis(100)).unwrap(),
        )
        .unwrap();

        let error = durable_domain_event_consumer_config(&event_store, &consumer).unwrap_err();

        assert_eq!(
            error.kind(),
            DomainEventConsumerErrorKind::InvalidConfiguration
        );

        let other_context_consumer = NatsDomainEventConsumerConfig::new(
            ConsumerName::new("acme", "billing", "projection", 1).unwrap(),
            DurableName::new("acme", "billing", "projection", 1).unwrap(),
            Duration::from_secs(5),
            Duration::from_secs(2),
            RetryDelay::new(Duration::from_millis(100)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            durable_domain_event_consumer_config(&event_store, &other_context_consumer)
                .unwrap_err()
                .kind(),
            DomainEventConsumerErrorKind::InvalidConfiguration
        );
    }

    fn stream_id(id: &str) -> StreamId {
        StreamId::new(
            AggregateType::new("test").unwrap(),
            AggregateId::new(id).unwrap(),
        )
    }

    fn buffered_event(
        stream_id: &StreamId,
        stream_version: u64,
        event_ordinal: usize,
        event_count: usize,
        transaction_event_ordinal: usize,
        transaction_event_count: usize,
        stream_sequence: u64,
    ) -> BufferedDomainEvent {
        let operation_id = OperationId::new("transaction-operation").unwrap();
        let operation_fingerprint = ContentFingerprint::digest("transaction-operation");
        let metadata = ExecutionMetadata::new(
            stream_id.clone(),
            operation_id.clone(),
            operation_fingerprint,
        );
        let commit_event_ordinal = u32::try_from(event_ordinal).unwrap();
        let commit_event_count = u32::try_from(event_count).unwrap();
        let recorded = RecordedEvent::new_in_commit(
            stream_id.clone(),
            StreamVersion::new(stream_version),
            metadata.event_id(commit_event_ordinal),
            metadata.commit_id().clone(),
            operation_id.clone(),
            operation_fingerprint,
            commit_event_ordinal,
            commit_event_count,
            "test-event",
            1,
            Vec::new(),
        )
        .unwrap();
        BufferedDomainEvent {
            stream_sequence,
            decoded: DecodedEvent {
                batch_id: "transaction-batch".to_owned(),
                commit_id: metadata.commit_id().clone(),
                operation_id,
                operation_fingerprint,
                event_ordinal,
                event_count,
                transaction_event_ordinal,
                transaction_event_count,
                is_transactional: false,
                recorded,
            },
        }
    }
}
