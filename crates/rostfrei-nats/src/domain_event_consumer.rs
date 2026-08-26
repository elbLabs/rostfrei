use std::{sync::Arc, time::Duration};

use async_nats::jetstream::{
    self,
    consumer::{self, AckPolicy, DeliverPolicy},
    message::AckKind,
};
use futures_util::TryStreamExt;
use rostfrei_core::{
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandlerError,
    DomainEventHandlerErrorKind, EventStoreErrorKind, MAX_EVENTS_PER_BATCH,
};
use rostfrei_messaging_core::{ConsumerName, DurableName, RetryDelay, MAX_PROCESSING_TIMEOUT};
use thiserror::Error;
use tokio::{sync::watch, time::timeout};

use crate::{
    event_store::{decode_consumed_event, DecodedEvent, NatsEventStore},
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
    event_store: NatsEventStoreConfig,
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
        NatsEventStore::connect(context.clone(), event_store.clone())
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
            .get_stream(event_store.stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        let consumer: consumer::PullConsumer = stream
            .get_consumer(config.durable_name().as_str())
            .await
            .map_err(|error| unavailable(format!("failed to get durable consumer: {error}")))?;
        verify_consumer(&consumer, &event_store, &config)?;
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
            .get_stream(self.event_store.stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        let consumer: consumer::PullConsumer = stream
            .get_consumer(self.config.durable_name().as_str())
            .await
            .map_err(|error| unavailable(format!("failed to get durable consumer: {error}")))?;
        verify_consumer(&consumer, &self.event_store, &self.config)?;
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
                message = first_delivery.try_next() => match message
                    .map_err(|error| unavailable(format!("domain-event delivery failed: {error}")))?
                {
                    Some(message) => message,
                    None => continue,
                },
            };
            let first = self.buffer_delivery(first)?;
            let consumer_info = consumer.get_info().await.map_err(|error| {
                unavailable(format!("failed to inspect durable progress: {error}"))
            })?;
            let expected_sequence = consumer_info
                .ack_floor
                .stream_sequence
                .checked_add(1)
                .ok_or_else(|| invalid_committed_event("durable sequence space overflowed"))?;
            if first.stream_sequence != expected_sequence {
                tracing::warn!(
                    durable = %self.config.durable_name(),
                    expected_sequence,
                    delivered_sequence = first.stream_sequence,
                    "deferring delivery until the durable's earliest unresolved event is available"
                );
                defer_delivery(&first, self.config.ack_wait()).await?;
                if !wait_for_retry(self.config.ack_wait(), &mut shutdown).await {
                    return Ok(());
                }
                continue;
            }

            let mut commit = self
                .reconstruct_acknowledged_prefix(&stream, &first)
                .await?;
            if commit.is_empty() && first.decoded.event_ordinal != 0 {
                return Err(invalid_committed_event(
                    "durable delivery started inside a committed event batch",
                ));
            }
            if !commit.is_empty() {
                validate_next_event(&commit, &first)?;
            }
            let event_count = first.decoded.event_count;
            commit.push(first);
            let remaining = event_count as usize - commit.len();
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
                while commit.len() < event_count as usize {
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
                    validate_next_event(&commit, &next)?;
                    commit.push(next);
                }
            }

            match timeout(
                self.config.processing_timeout(),
                self.handle_commit(&commit),
            )
            .await
            {
                Ok(Ok(())) => acknowledge_commit(&commit).await?,
                Ok(Err(error)) if error.kind() == DomainEventHandlerErrorKind::Retryable => {
                    tracing::warn!(
                        durable = %self.config.durable_name(),
                        commit_id = %commit[0].decoded.commit_id,
                        error = %error,
                        "domain-event handler requested redelivery"
                    );
                    let delay = self.config.retry_delay().get();
                    retry_commit(&commit, delay).await?;
                    if !wait_for_retry(delay, &mut shutdown).await {
                        return Ok(());
                    }
                }
                Ok(Err(error)) => {
                    tracing::error!(
                        durable = %self.config.durable_name(),
                        commit_id = %commit[0].decoded.commit_id,
                        error = %error,
                        "domain-event durable is blocked and requires operator action"
                    );
                    return Err(handler_blocked(&error));
                }
                Err(_) => {
                    tracing::warn!(
                        durable = %self.config.durable_name(),
                        commit_id = %commit[0].decoded.commit_id,
                        "domain-event handler timed out and will be redelivered"
                    );
                    let delay = self.config.retry_delay().get();
                    retry_commit(&commit, delay).await?;
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
    ) -> Result<BufferedDomainEvent, DomainEventConsumerError> {
        let info = message
            .info()
            .map_err(|error| unavailable(format!("delivery metadata is unavailable: {error}")))?;
        if info.stream != self.event_store.stream_name()
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
            &self.event_store,
            message.subject.as_str(),
            headers,
            &message.payload,
        )
        .map_err(|error| invalid_committed_event(error.to_string()))?;
        Ok(BufferedDomainEvent {
            delivery: Some(message),
            stream_sequence,
            decoded,
        })
    }

    async fn reconstruct_acknowledged_prefix(
        &self,
        stream: &jetstream::stream::Stream,
        first: &BufferedDomainEvent,
    ) -> Result<Vec<BufferedDomainEvent>, DomainEventConsumerError> {
        if first.decoded.event_ordinal == 0 {
            return Ok(Vec::new());
        }
        let start_sequence = first
            .stream_sequence
            .checked_sub(u64::from(first.decoded.event_ordinal))
            .ok_or_else(|| invalid_committed_event("commit start sequence underflowed"))?;
        let mut prefix = Vec::with_capacity(first.decoded.event_ordinal as usize);
        for sequence in start_sequence..first.stream_sequence {
            let raw = stream.get_raw_message(sequence).await.map_err(|error| {
                unavailable(format!(
                    "failed to reconstruct acknowledged commit prefix: {error}"
                ))
            })?;
            let decoded = decode_consumed_event(
                &self.event_store,
                raw.subject.as_str(),
                &raw.headers,
                &raw.payload,
            )
            .map_err(|error| invalid_committed_event(error.to_string()))?;
            let event = BufferedDomainEvent {
                delivery: None,
                stream_sequence: raw.sequence,
                decoded,
            };
            if prefix.is_empty() {
                if event.decoded.event_ordinal != 0 {
                    return Err(invalid_committed_event(
                        "acknowledged commit prefix does not start at ordinal zero",
                    ));
                }
            } else {
                validate_next_event(&prefix, &event)?;
            }
            prefix.push(event);
        }
        Ok(prefix)
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
    delivery: Option<jetstream::Message>,
    stream_sequence: u64,
    decoded: DecodedEvent,
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
    let first = &commit[0].decoded;
    let previous = commit.last().expect("commit has a first event");
    let expected_ordinal = u32::try_from(commit.len())
        .map_err(|_| invalid_committed_event("commit ordinal cannot be represented"))?;
    if next.decoded.event_ordinal != expected_ordinal
        || next.decoded.event_count != first.event_count
        || next.decoded.batch_id != first.batch_id
        || next.decoded.commit_id != first.commit_id
        || next.decoded.operation_id != first.operation_id
        || next.decoded.operation_fingerprint != first.operation_fingerprint
        || next.decoded.recorded.stream_id() != first.recorded.stream_id()
        || next.decoded.recorded.correlation_id() != first.recorded.correlation_id()
        || next.decoded.recorded.causation_id() != first.recorded.causation_id()
        || next.decoded.recorded.stream_version().value()
            != previous
                .decoded
                .recorded
                .stream_version()
                .value()
                .checked_add(1)
                .unwrap_or(0)
        || next.stream_sequence != previous.stream_sequence.checked_add(1).unwrap_or(0)
    {
        return Err(invalid_committed_event(
            "committed event batch is missing, reordered, or inconsistent",
        ));
    }
    Ok(())
}

async fn acknowledge_commit(
    commit: &[BufferedDomainEvent],
) -> Result<(), DomainEventConsumerError> {
    commit
        .last()
        .expect("commit is non-empty")
        .delivery
        .as_ref()
        .expect("the final committed event has a live delivery")
        .double_ack()
        .await
        .map_err(|error| {
            DomainEventConsumerError::new(
                DomainEventConsumerErrorKind::Acknowledgement,
                format!("failed to acknowledge committed event batch: {error}"),
            )
        })
}

async fn retry_commit(
    commit: &[BufferedDomainEvent],
    delay: Duration,
) -> Result<(), DomainEventConsumerError> {
    for event in commit {
        let Some(message) = &event.delivery else {
            continue;
        };
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
    event: &BufferedDomainEvent,
    delay: Duration,
) -> Result<(), DomainEventConsumerError> {
    event
        .delivery
        .as_ref()
        .expect("buffered delivery is live")
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

fn invalid_committed_event(message: impl Into<String>) -> DomainEventConsumerError {
    DomainEventConsumerError::new(DomainEventConsumerErrorKind::InvalidCommittedEvent, message)
}
