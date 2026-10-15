use std::{sync::Arc, time::Duration};

use async_nats::jetstream::{
    self,
    consumer::{self, AckPolicy, DeliverPolicy},
    message::AckKind,
    stream::{RawMessageError, RawMessageErrorKind},
};
use futures_util::{StreamExt, TryStreamExt, stream};
use rostfrei_core::{
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandlerError,
    DomainEventHandlerErrorKind, EventStore, EventStoreErrorKind, RecordedEvent,
};
use rostfrei_fixtures::FixtureEventSet;
use rostfrei_messaging_core::{ConsumerName, DurableName, MAX_PROCESSING_TIMEOUT, RetryDelay};
use thiserror::Error;
use tokio::{
    sync::{RwLock, watch},
    time::timeout,
};

use crate::{
    event_store::{DecodedEvent, NatsEventStore, decode_consumed_event},
    event_store_config::NatsEventStoreConfig,
};

const PULL_EXPIRATION: Duration = Duration::from_secs(1);
const MAX_WAITING_PULLS: i64 = 512;
const HISTORY_READ_CONCURRENCY: usize = 16;

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
    start_sequence: Option<u64>,
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
            || name.traffic_scope() != durable_name.traffic_scope()
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
            start_sequence: None,
        })
    }

    /// Initial position for an explicitly migrated durable. Stop all old workers
    /// first and use the saved confirmed stream sequence plus one. Keep this
    /// setting on subsequent starts; it does not reset an existing consumer.
    pub fn with_start_sequence(mut self, sequence: u64) -> Result<Self, DomainEventConsumerError> {
        if sequence == 0 {
            return Err(invalid_configuration(
                "initial stream sequence must be nonzero",
            ));
        }
        self.start_sequence = Some(sequence);
        Ok(self)
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
    fixture_events: Option<Arc<RwLock<FixtureEventSet>>>,
}

impl NatsDomainEventConsumer {
    pub async fn connect(
        context: jetstream::Context,
        event_store: NatsEventStoreConfig,
        config: NatsDomainEventConsumerConfig,
        dispatcher: Arc<DomainEventDispatcher>,
    ) -> Result<Self, DomainEventConsumerError> {
        Self::connect_inner(context, event_store, config, dispatcher, None).await
    }

    /// Connects a consumer that acknowledges exact fixture-generated direct
    /// appends without dispatching them. The fixture set must be updated while
    /// holding its write lock across fixture persistence so delivery cannot
    /// observe partially registered fixture state.
    pub async fn connect_with_fixture_events(
        context: jetstream::Context,
        event_store: NatsEventStoreConfig,
        config: NatsDomainEventConsumerConfig,
        dispatcher: Arc<DomainEventDispatcher>,
        fixture_events: Arc<RwLock<FixtureEventSet>>,
    ) -> Result<Self, DomainEventConsumerError> {
        Self::connect_inner(
            context,
            event_store,
            config,
            dispatcher,
            Some(fixture_events),
        )
        .await
    }

    async fn connect_inner(
        context: jetstream::Context,
        event_store: NatsEventStoreConfig,
        config: NatsDomainEventConsumerConfig,
        dispatcher: Arc<DomainEventDispatcher>,
        fixture_events: Option<Arc<RwLock<FixtureEventSet>>>,
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
            fixture_events,
        })
    }

    pub async fn run_until_shutdown(
        self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), DomainEventConsumerError> {
        if shutdown_requested(&shutdown) {
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
        while !shutdown_requested(&shutdown) {
            let mut deliveries = consumer
                .batch()
                .max_messages(1)
                .expires(PULL_EXPIRATION)
                .messages()
                .await
                .map_err(|error| {
                    unavailable(format!("failed to start durable delivery: {error}"))
                })?;
            // Let this finite pull finish even on shutdown. Dropping its inbox
            // early can strand a delivery until AckWait. A late arrival is NAKed
            // below without starting another transaction.
            let Some(delivery) = deliveries
                .try_next()
                .await
                .map_err(|error| unavailable(format!("domain-event delivery failed: {error}")))?
            else {
                continue;
            };
            drop(deliveries);
            if shutdown_requested(&shutdown) {
                defer_delivery(&delivery, Duration::ZERO).await?;
                break;
            }
            let LiveDomainEvent { event, delivery } = self.buffer_delivery(delivery)?;
            let result = timeout(
                self.config.processing_timeout(),
                self.process_event(&stream, &consumer, event),
            )
            .await
            .unwrap_or_else(|_| Err(unavailable("domain-event transaction processing timed out")));
            match result {
                Ok(true) => {
                    acknowledge_delivery(&delivery).await?;
                    continue;
                }
                Ok(false) => {}
                Err(error) if error.kind() == DomainEventConsumerErrorKind::Unavailable => {
                    tracing::warn!(durable = %self.config.durable_name(), %error,
                        "domain-event transaction will be retried");
                }
                Err(error) => return Err(error),
            }
            let delay = if shutdown_requested(&shutdown) {
                Duration::ZERO
            } else {
                self.config.retry_delay().get()
            };
            // The processing future has ended before work is released.
            defer_delivery(&delivery, delay).await?;
            if !wait_for_retry(delay, &mut shutdown).await {
                break;
            }
        }
        Ok(())
    }

    /// With one pending message across the durable, its first event represents
    /// the whole transaction's work. Its ACK certifies that *all* handlers
    /// succeeded. Later events only drain the durable's existing ACK state,
    /// even if another worker receives them or the first worker has restarted.
    async fn process_event(
        &self,
        stream: &jetstream::stream::Stream,
        consumer: &consumer::PullConsumer,
        event: BufferedDomainEvent,
    ) -> Result<bool, DomainEventConsumerError> {
        let info = consumer
            .get_info()
            .await
            .map_err(|error| unavailable(format!("failed to inspect durable progress: {error}")))?;
        let migrated_floor = self
            .config
            .start_sequence
            .and_then(|sequence| sequence.checked_sub(1))
            .unwrap_or(0);
        let ack_floor = info.ack_floor.stream_sequence.max(migrated_floor);
        if event.stream_sequence <= ack_floor {
            return Ok(true); // An old delivery whose retry has already completed.
        }
        let start = event.transaction_start()?;
        if start <= ack_floor {
            // Check the immutable first event before interpreting its ACK as
            // proof. A forged ordinal must not borrow another transaction's ACK.
            let first = self.read_event(stream, start).await?;
            validate_acknowledged_member(&first, &event)?;
            return Ok(true);
        }
        let search_start = ack_floor
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
        if event.stream_sequence != expected_sequence {
            return Ok(false);
        }
        let transaction = self.reconstruct_transaction(stream, event).await?;
        self.validate_transaction_receipt(&transaction).await?;
        self.handle_transaction(&transaction)
            .await
            .map_err(|error| handler_blocked(&error))?;
        Ok(true)
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

    async fn read_event(
        &self,
        stream: &jetstream::stream::Stream,
        sequence: u64,
    ) -> Result<BufferedDomainEvent, DomainEventConsumerError> {
        let raw = stream
            .get_raw_message(sequence)
            .await
            .map_err(|error| transaction_history_lookup_error(&error))?;
        let decoded = decode_consumed_event(
            self.event_store.config(),
            raw.subject.as_str(),
            &raw.headers,
            &raw.payload,
        )
        .map_err(|error| invalid_committed_event(error.to_string()))?;
        Ok(BufferedDomainEvent {
            stream_sequence: raw.sequence,
            decoded,
        })
    }

    async fn reconstruct_transaction(
        &self,
        stream: &jetstream::stream::Stream,
        first: BufferedDomainEvent,
    ) -> Result<BufferedEventTransaction, DomainEventConsumerError> {
        validate_first_event(&first)?;
        let count = u64::try_from(first.decoded.transaction_event_count).map_err(|_| {
            invalid_committed_event("transaction event count cannot be represented")
        })?;
        let end = first
            .stream_sequence
            .checked_add(count)
            .ok_or_else(|| invalid_committed_event("transaction sequence space overflowed"))?;
        let next = first
            .stream_sequence
            .checked_add(1)
            .ok_or_else(|| invalid_committed_event("transaction sequence space overflowed"))?;
        let mut events = vec![first];
        // Read committed history, not more work from the shared durable. Bounded
        // concurrent reads avoid one broker roundtrip per event in a large batch.
        let mut remaining = stream::iter(next..end)
            .map(|sequence| self.read_event(stream, sequence))
            .buffered(HISTORY_READ_CONCURRENCY);
        while let Some(event) = remaining.try_next().await? {
            validate_next_event(&events, &event)?;
            events.push(event);
        }
        BufferedEventTransaction::try_new(events)
    }

    async fn validate_transaction_receipt(
        &self,
        transaction: &BufferedEventTransaction,
    ) -> Result<(), DomainEventConsumerError> {
        let first = transaction
            .first()
            .ok_or_else(|| invalid_committed_event("committed transaction is empty"))?;
        if transaction
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
            .load_transaction_receipt(&first.decoded.operation_id)
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
        if receipt_events.len() != transaction.len()
            || receipt_events
                .iter()
                .zip(transaction.iter())
                .any(|(receipt_event, buffered)| receipt_event != &buffered.decoded.recorded)
        {
            return Err(invalid_committed_event(
                "committed transactional events do not match their durable receipt",
            ));
        }
        Ok(())
    }

    async fn handle_transaction(
        &self,
        transaction: &BufferedEventTransaction,
    ) -> Result<(), DomainEventHandlerError> {
        if let Some(fixture_events) = &self.fixture_events
            && !transaction.is_transactional()
        {
            let fixture_events = fixture_events.read().await;
            let fixture_event_count = transaction
                .recorded_events()
                .filter(|event| fixture_events.contains(event))
                .count();
            if fixture_event_count == transaction.len() {
                return Ok(());
            }
            if fixture_event_count != 0 {
                return Err(DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::InvalidCommittedEvent,
                    "committed event transaction mixes fixture and application events",
                ));
            }
        }

        for event in transaction.iter() {
            match self.dispatcher.dispatch(&event.decoded.recorded).await? {
                DomainEventDispatchOutcome::Handled | DomainEventDispatchOutcome::Ignored => {}
            }
        }
        Ok(())
    }
}

struct BufferedEventTransaction {
    events: Vec<BufferedDomainEvent>,
}

impl BufferedEventTransaction {
    fn try_new(events: Vec<BufferedDomainEvent>) -> Result<Self, DomainEventConsumerError> {
        validate_complete_transaction(&events)?;
        Ok(Self { events })
    }

    fn first(&self) -> Option<&BufferedDomainEvent> {
        self.events.first()
    }

    const fn len(&self) -> usize {
        self.events.len()
    }

    fn is_transactional(&self) -> bool {
        self.first()
            .is_some_and(|event| event.decoded.is_transactional)
    }

    fn iter(&self) -> impl ExactSizeIterator<Item = &BufferedDomainEvent> {
        self.events.iter()
    }

    fn recorded_events(&self) -> impl ExactSizeIterator<Item = &RecordedEvent> {
        self.events.iter().map(|event| &event.decoded.recorded)
    }
}

struct BufferedDomainEvent {
    stream_sequence: u64,
    decoded: DecodedEvent,
}

impl BufferedDomainEvent {
    fn transaction_start(&self) -> Result<u64, DomainEventConsumerError> {
        let ordinal = u64::try_from(self.decoded.transaction_event_ordinal)
            .map_err(|_| invalid_committed_event("transaction ordinal cannot be represented"))?;
        self.stream_sequence
            .checked_sub(ordinal)
            .filter(|sequence| *sequence > 0)
            .ok_or_else(|| invalid_committed_event("transaction start sequence underflowed"))
    }
}

struct LiveDomainEvent {
    event: BufferedDomainEvent,
    delivery: jetstream::Message,
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
        || config.name().traffic_scope() != event_store.traffic_scope()
        || config.durable_name().traffic_scope() != event_store.traffic_scope()
    {
        return Err(invalid_configuration(
            "domain-event consumer belongs to a different application or bounded context",
        ));
    }
    Ok(consumer::pull::Config {
        durable_name: Some(config.durable_name().as_str().to_owned()),
        name: Some(config.durable_name().as_str().to_owned()),
        description: Some(config.name().as_str().to_owned()),
        deliver_policy: config
            .start_sequence
            .map_or(DeliverPolicy::All, |start_sequence| {
                DeliverPolicy::ByStartSequence { start_sequence }
            }),
        ack_policy: AckPolicy::Explicit,
        ack_wait: config.ack_wait(),
        max_deliver: -1,
        filter_subject: event_store.aggregate_subject_filter(),
        // Ordered work sharing: only the oldest delivery may drive effects.
        // Its transaction tail is read directly from immutable stream history.
        max_ack_pending: 1,
        max_batch: 1,
        max_waiting: MAX_WAITING_PULLS,
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
    if actual.ack_policy == AckPolicy::All || actual.max_waiting == 1 {
        return Err(invalid_configuration(
            "legacy domain-event durable requires offline migration: stop its workers, save the \
             confirmed stream sequence, and recreate with Explicit ACKs and shared waiting pulls",
        ));
    }
    if consumer.cached_info().name != config.durable_name().as_str()
        || actual.deliver_subject.is_some()
        || actual.deliver_group.is_some()
        || actual.name.as_deref() != expected.name.as_deref()
        || actual.durable_name.as_deref() != expected.durable_name.as_deref()
        || actual.description != expected.description
        || actual.deliver_policy != expected.deliver_policy
        || actual.ack_policy != AckPolicy::Explicit
        || actual.ack_wait != expected.ack_wait
        || actual.max_deliver != -1
        || actual.filter_subject != expected.filter_subject
        || actual.replay_policy != expected.replay_policy
        || actual.rate_limit != expected.rate_limit
        || actual.sample_frequency != expected.sample_frequency
        || actual.max_ack_pending != expected.max_ack_pending
        || actual.max_batch != expected.max_batch
        || actual.max_waiting != expected.max_waiting
        || actual.headers_only != expected.headers_only
        || actual.flow_control
        || !actual.idle_heartbeat.is_zero()
        || actual.max_bytes != expected.max_bytes
        || actual.max_expires != expected.max_expires
        || actual.inactive_threshold != expected.inactive_threshold
        || actual.num_replicas != expected.num_replicas
        || actual.memory_storage != expected.memory_storage
        || actual.backoff != expected.backoff
        || actual.priority_policy != expected.priority_policy
        || actual.priority_groups != expected.priority_groups
    {
        return Err(invalid_configuration(
            "existing durable consumer configuration does not match",
        ));
    }
    Ok(())
}

fn validate_acknowledged_member(
    first: &BufferedDomainEvent,
    event: &BufferedDomainEvent,
) -> Result<(), DomainEventConsumerError> {
    validate_first_event(first)?;
    if event.transaction_start()? != first.stream_sequence
        || event.decoded.transaction_event_count != first.decoded.transaction_event_count
        || event.decoded.batch_id != first.decoded.batch_id
        || event.decoded.operation_id != first.decoded.operation_id
        || event.decoded.operation_fingerprint != first.decoded.operation_fingerprint
        || event.decoded.is_transactional != first.decoded.is_transactional
        || event.decoded.recorded.correlation_id() != first.decoded.recorded.correlation_id()
        || event.decoded.recorded.causation_id() != first.decoded.recorded.causation_id()
    {
        return Err(invalid_committed_event(
            "event does not belong to the acknowledged transaction",
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

async fn acknowledge_delivery(
    delivery: &jetstream::Message,
) -> Result<(), DomainEventConsumerError> {
    delivery.double_ack().await.map_err(|error| {
        DomainEventConsumerError::new(
            DomainEventConsumerErrorKind::Acknowledgement,
            format!("failed to acknowledge domain-event delivery: {error}"),
        )
    })
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
                format!("failed to release domain-event delivery for redelivery: {error}"),
            )
        })
}

async fn wait_for_retry(delay: Duration, shutdown: &mut watch::Receiver<bool>) -> bool {
    if shutdown_requested(shutdown) {
        return false;
    }
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

fn shutdown_requested(shutdown: &watch::Receiver<bool>) -> bool {
    *shutdown.borrow() || shutdown.has_changed().is_err()
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

fn transaction_history_lookup_error(error: &RawMessageError) -> DomainEventConsumerError {
    raw_message_lookup_error(
        error,
        "committed transaction",
        "reconstruct committed transaction",
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
        AggregateId, AggregateType, ContentFingerprint, OperationId, RecordedEvent, StreamId,
        StreamVersion, derive_commit_id, derive_event_id,
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
    fn transaction_history_lookup_distinguishes_missing_history_from_unavailability() {
        let missing = transaction_history_lookup_error(&RawMessageError::new(
            RawMessageErrorKind::NoMessageFound,
        ));
        let unavailable =
            transaction_history_lookup_error(&RawMessageError::new(RawMessageErrorKind::Other));

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
    fn a_tail_cannot_borrow_another_transactions_acknowledgement() {
        let source = stream_id("source");
        let first = buffered_event(&source, 1, 0, 1, 0, 1, 1);
        let mut forged = buffered_event(&source, 2, 1, 2, 1, 2, 2);
        assert!(validate_acknowledged_member(&first, &forged).is_err());

        let first = buffered_event(&source, 1, 0, 2, 0, 2, 1);
        assert!(validate_acknowledged_member(&first, &forged).is_ok());
        forged.decoded.batch_id = "another-atomic-batch".to_owned();
        assert!(validate_acknowledged_member(&first, &forged).is_err());
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
        let commit_id = derive_commit_id(stream_id, &operation_id);
        let commit_event_ordinal = u32::try_from(event_ordinal).unwrap();
        let commit_event_count = u32::try_from(event_count).unwrap();
        let recorded = RecordedEvent::new_in_commit(
            stream_id.clone(),
            StreamVersion::new(stream_version),
            derive_event_id(&commit_id, commit_event_ordinal),
            commit_id.clone(),
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
                commit_id,
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
