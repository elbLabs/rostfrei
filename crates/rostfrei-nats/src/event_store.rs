use std::collections::HashSet;
use std::fmt::Write as _;

use async_nats::{
    HeaderMap, Request,
    header::{
        NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL, NATS_BATCH_ID, NATS_BATCH_SEQUENCE,
        NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, NATS_EXPECTED_STREAM, NATS_REQUIRED_API_LEVEL,
    },
    jetstream::{
        self,
        response::Response,
        stream::{Config, LastRawMessageErrorKind},
    },
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, CommitId, ContentFingerprint, EventBatch,
    EventHistory, EventId, EventStore, EventStoreError, EventStoreErrorKind, ExecutionMetadata,
    ExpectedVersion, MAX_EVENTS_PER_BATCH, NewEvent, OperationId, RecordedEvent, StreamId,
    StreamVersion,
};
use rostfrei_messaging_core::{CausationId, CorrelationId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::event_store_config::NatsEventStoreConfig;
use crate::stream_policy::{is_stream_not_found, stream_config_mismatches};

const LEGACY_EVENT_SCHEMA_VERSION: u16 = 1;
const CORRELATION_EVENT_SCHEMA_VERSION: u16 = 2;
const EVENT_SCHEMA_VERSION: u16 = 3;
const ATOMIC_BATCH_API_LEVEL: &str = "2";
const MINIMUM_ATOMIC_BATCH_SERVER_VERSION: (i64, i64, i64) = (2, 12, 0);

#[derive(Clone)]
pub struct NatsEventStore {
    context: jetstream::Context,
    config: NatsEventStoreConfig,
}

impl NatsEventStore {
    pub async fn connect(
        context: jetstream::Context,
        config: NatsEventStoreConfig,
    ) -> Result<Self, EventStoreError> {
        let (major, minor, patch) = MINIMUM_ATOMIC_BATCH_SERVER_VERSION;
        if !context.client().is_server_compatible(major, minor, patch) {
            return Err(EventStoreError::new(
                EventStoreErrorKind::ConfigurationMismatch,
                "NATS Server 2.12.0 or newer is required for atomic event batches",
            ));
        }
        let stream = context
            .get_stream(config.stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        verify_stream_config(&config.stream_config(), &stream.cached_info().config)?;
        Ok(Self { context, config })
    }

    pub fn config(&self) -> &NatsEventStoreConfig {
        &self.config
    }

    pub async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        <Self as EventHistory>::load(self, stream_id).await
    }

    async fn load_history(&self, stream_id: &StreamId) -> Result<History, EventStoreError> {
        let subject = self.config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let stream = self
            .context
            .get_stream(self.config.stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        let last_sequence = match stream.get_last_raw_message_by_subject(&subject).await {
            Ok(message) => {
                if message.subject.as_str() != subject {
                    return Err(corrupt("aggregate lookup returned the wrong subject"));
                }
                message.sequence
            }
            Err(error) if error.kind() == LastRawMessageErrorKind::NoMessageFound => {
                return Ok(History::default());
            }
            Err(error) => {
                return Err(unavailable(format!(
                    "failed to locate aggregate history: {error}"
                )));
            }
        };

        let mut history = HistoryBuilder::default();
        let mut next_stream_sequence = 1_u64;
        let mut last_commit_stream_sequence = 0_u64;

        while next_stream_sequence <= last_sequence {
            let message = stream
                .get_first_raw_message_by_subject(&subject, next_stream_sequence)
                .await
                .map_err(|error| match error.kind() {
                    LastRawMessageErrorKind::NoMessageFound => {
                        corrupt("aggregate history disappeared while loading")
                    }
                    _ => unavailable(format!("failed to read aggregate history: {error}")),
                })?;
            if message.subject.as_str() != subject {
                return Err(corrupt("aggregate history contains the wrong subject"));
            }
            if message.sequence < next_stream_sequence || message.sequence > last_sequence {
                return Err(corrupt(
                    "aggregate history returned an invalid stream sequence",
                ));
            }

            let decoded = decode_event(
                &self.config,
                &subject,
                stream_id,
                Some(last_commit_stream_sequence),
                &message.headers,
                message.payload.as_ref(),
            )?;
            if decoded.event_ordinal + 1 == decoded.event_count {
                last_commit_stream_sequence = message.sequence;
            }
            history.push(decoded)?;

            if message.sequence == last_sequence {
                break;
            }
            next_stream_sequence = message
                .sequence
                .checked_add(1)
                .ok_or_else(|| corrupt("JetStream sequence space overflowed"))?;
        }

        if next_stream_sequence > last_sequence {
            return Err(corrupt("aggregate history ended before its last message"));
        }
        history.finish(last_sequence)
    }

    fn resolve_existing(
        history: &History,
        batch: &EventBatch,
    ) -> Result<Option<Vec<RecordedEvent>>, EventStoreError> {
        if let Some(previous) = history
            .commits
            .iter()
            .find(|commit| commit.batch.operation_id() == batch.operation_id())
        {
            if same_batch(&previous.batch, batch) {
                return Ok(Some(previous.events.clone()));
            }
            return Err(identity_conflict(
                "operation identity was reused with different content",
            ));
        }
        if history
            .commits
            .iter()
            .any(|commit| commit.batch.commit_id() == batch.commit_id())
        {
            return Err(identity_conflict(
                "commit identity was reused with different content",
            ));
        }
        if batch.events().iter().any(|incoming| {
            history
                .events
                .iter()
                .any(|stored| stored.event_id() == incoming.event_id())
        }) {
            return Err(identity_conflict(
                "event identity was reused with different content",
            ));
        }
        Ok(None)
    }

    async fn resolve_expectation_race(
        &self,
        stream_id: &StreamId,
        batch: &EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        let history = self.load_history(stream_id).await?;
        match Self::resolve_existing(&history, batch)? {
            Some(events) => Ok(AppendOutcome::ExactReplay(events)),
            None => Err(conflict("aggregate changed during append")),
        }
    }

    async fn verify_published_commit(
        &self,
        stream_id: &StreamId,
        subject: &str,
        sequence: u64,
        commit_id: &CommitId,
        expected_events: &[RecordedEvent],
    ) -> Result<(), EventStoreError> {
        let history = self.load_history(stream_id).await?;
        let stored = history
            .commits
            .iter()
            .find(|commit| commit.batch.commit_id() == commit_id)
            .ok_or_else(|| corrupt("published commit was not visible in aggregate history"))?;
        if stored.events != expected_events {
            return Err(corrupt("published commit contains different events"));
        }

        let stream = self
            .context
            .get_stream(self.config.stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to verify PubAck stream: {error}")))?;
        let message = stream
            .get_first_raw_message_by_subject(subject, sequence)
            .await
            .map_err(|error| unavailable(format!("failed to verify published commit: {error}")))?;
        if message.sequence != sequence || message.subject.as_str() != subject {
            return Err(corrupt(
                "PubAck sequence did not identify the published commit",
            ));
        }
        let decoded = decode_event(
            &self.config,
            subject,
            stream_id,
            None,
            &message.headers,
            message.payload.as_ref(),
        )?;
        if decoded.recorded != *expected_events.last().expect("event batch is non-empty") {
            return Err(corrupt("PubAck sequence contains a different final event"));
        }
        Ok(())
    }
}

#[async_trait]
impl EventHistory for NatsEventStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        Ok(self.load_history(stream_id).await?.events)
    }
}

#[async_trait]
impl EventStore for NatsEventStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        let history = self.load_history(stream_id).await?;
        if let Some(events) = Self::resolve_existing(&history, &batch)? {
            return Ok(AppendOutcome::ExactReplay(events));
        }
        if expected_version == ExpectedVersion::Exact(StreamVersion::ZERO) {
            return Err(invalid(
                "Exact requires a non-zero stream version; use NoStream for an absent stream",
            ));
        }
        validate_derived_identities(stream_id, &batch)?;

        let current_version = history
            .events
            .last()
            .map_or(StreamVersion::ZERO, RecordedEvent::stream_version);
        match expected_version {
            ExpectedVersion::NoStream if current_version == StreamVersion::ZERO => {}
            ExpectedVersion::Exact(version) if version == current_version => {}
            ExpectedVersion::NoStream | ExpectedVersion::Exact(_) => {
                return Err(conflict(format!(
                    "expected version does not match current version {}",
                    current_version.value()
                )));
            }
        }

        let recorded = record_batch(stream_id, current_version, &batch)?;
        let subject = self.config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let payloads = encode_events(&self.config, stream_id, &batch, &recorded)?;
        for payload in &payloads {
            if payload.len() > self.config.max_event_bytes() {
                return Err(invalid(format!(
                    "encoded event exceeds the configured {}-byte limit",
                    self.config.max_event_bytes()
                )));
            }
        }
        let batch_id = new_atomic_batch_id(&self.context.client(), batch.commit_id());
        let ack = match publish_atomic_batch(
            &self.context,
            &self.config,
            &subject,
            history.last_subject_stream_sequence,
            &batch_id,
            payloads,
        )
        .await
        {
            Ok(ack) => ack,
            Err(AtomicBatchPublishError::Expectation) => {
                return self.resolve_expectation_race(stream_id, &batch).await;
            }
            Err(AtomicBatchPublishError::Store(error)) => return Err(error),
        };
        if ack.stream != self.config.stream_name()
            || ack.sequence == 0
            || ack.sequence <= history.last_subject_stream_sequence
            || ack.batch.as_deref() != Some(batch_id.as_str())
            || ack.count != Some(recorded.len() as u64)
        {
            return Err(corrupt(
                "atomic PubAck returned incompatible stream, sequence, or batch metadata",
            ));
        }
        self.verify_published_commit(
            stream_id,
            &subject,
            ack.sequence,
            batch.commit_id(),
            &recorded,
        )
        .await?;
        Ok(AppendOutcome::Appended(recorded))
    }
}

pub async fn provision_event_store(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
) -> Result<(), EventStoreError> {
    let expected = config.stream_config();
    match context.get_stream(config.stream_name()).await {
        Ok(existing) => {
            if existing.cached_info().config.subjects != expected.subjects {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::ConfigurationMismatch,
                    "existing event-store stream belongs to a different application or bounded context",
                ));
            }
        }
        Err(error) if is_stream_not_found(&error) => {}
        Err(error) => {
            return Err(unavailable(format!(
                "failed to inspect event-store stream before provisioning: {error}"
            )));
        }
    }
    let provisioned = context
        .create_or_update_stream(expected.clone())
        .await
        .map_err(|error| unavailable(format!("failed to provision event-store stream: {error}")))?;
    verify_stream_config(&expected, &provisioned.config)
}

#[derive(Default)]
struct History {
    events: Vec<RecordedEvent>,
    commits: Vec<StoredCommit>,
    last_subject_stream_sequence: u64,
}

struct StoredCommit {
    batch: EventBatch,
    events: Vec<RecordedEvent>,
}

#[derive(Default)]
struct HistoryBuilder {
    history: History,
    current_version: StreamVersion,
    operation_ids: HashSet<OperationId>,
    commit_ids: HashSet<CommitId>,
    event_ids: HashSet<EventId>,
    pending: Option<PendingCommit>,
}

struct PendingCommit {
    batch_id: String,
    commit_id: CommitId,
    operation_id: OperationId,
    operation_fingerprint: ContentFingerprint,
    event_count: u32,
    events: Vec<RecordedEvent>,
}

pub(crate) struct DecodedEvent {
    pub(crate) batch_id: String,
    pub(crate) commit_id: CommitId,
    pub(crate) operation_id: OperationId,
    pub(crate) operation_fingerprint: ContentFingerprint,
    pub(crate) event_ordinal: u32,
    pub(crate) event_count: u32,
    pub(crate) recorded: RecordedEvent,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEventWire {
    schema_version: u16,
    checksum: String,
    event: StoredEventContentWire,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEventContentWire {
    event_store_stream: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    application: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bounded_context: Option<String>,
    stream: StreamIdentityWire,
    stream_version: u64,
    commit_id: String,
    operation_id: String,
    operation_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    causation_id: Option<String>,
    commit_event_ordinal: u32,
    commit_event_count: u32,
    event_id: String,
    event_type: String,
    event_schema_version: u32,
    payload_base64: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StreamIdentityWire {
    aggregate_type: String,
    aggregate_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChecksumInput<'a> {
    schema_version: u16,
    event: &'a StoredEventContentWire,
}

fn encode_events(
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    batch: &EventBatch,
    recorded: &[RecordedEvent],
) -> Result<Vec<Vec<u8>>, EventStoreError> {
    if recorded.len() != batch.events().len() {
        return Err(invalid("recorded event count does not match its batch"));
    }
    let event_count = u32::try_from(recorded.len())
        .map_err(|_| invalid("event batch count cannot be represented"))?;
    recorded
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let ordinal = u32::try_from(index)
                .map_err(|_| invalid("event batch ordinal cannot be represented"))?;
            let content = StoredEventContentWire {
                event_store_stream: config.stream_name().to_owned(),
                application: Some(config.application().as_str().to_owned()),
                bounded_context: Some(config.bounded_context().as_str().to_owned()),
                stream: StreamIdentityWire {
                    aggregate_type: stream_id.aggregate_type().as_str().to_owned(),
                    aggregate_id: stream_id.aggregate_id().as_str().to_owned(),
                },
                stream_version: event.stream_version().value(),
                commit_id: batch.commit_id().as_str().to_owned(),
                operation_id: batch.operation_id().as_str().to_owned(),
                operation_fingerprint: batch.operation_fingerprint().to_hex(),
                correlation_id: batch
                    .correlation_id()
                    .map(|identity| identity.as_str().to_owned()),
                causation_id: batch
                    .causation_id()
                    .map(|identity| identity.as_str().to_owned()),
                commit_event_ordinal: ordinal,
                commit_event_count: event_count,
                event_id: event.event_id().as_str().to_owned(),
                event_type: event.event_type().to_owned(),
                event_schema_version: event.schema_version(),
                payload_base64: STANDARD.encode(event.payload()),
            };
            let checksum = event_checksum(EVENT_SCHEMA_VERSION, &content)
                .map_err(|error| invalid(format!("failed to checksum event: {error}")))?;
            serde_json::to_vec(&StoredEventWire {
                schema_version: EVENT_SCHEMA_VERSION,
                checksum,
                event: content,
            })
            .map_err(|error| invalid(format!("failed to encode event: {error}")))
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn decode_event(
    config: &NatsEventStoreConfig,
    subject: &str,
    expected_stream_id: &StreamId,
    expected_last_subject_sequence: Option<u64>,
    headers: &HeaderMap,
    payload: &[u8],
) -> Result<DecodedEvent, EventStoreError> {
    decode_event_inner(
        config,
        subject,
        Some(expected_stream_id),
        expected_last_subject_sequence,
        headers,
        payload,
    )
}

#[allow(dead_code)]
pub(crate) fn decode_consumed_event(
    config: &NatsEventStoreConfig,
    subject: &str,
    headers: &HeaderMap,
    payload: &[u8],
) -> Result<DecodedEvent, EventStoreError> {
    decode_event_inner(config, subject, None, None, headers, payload)
}

#[allow(clippy::too_many_lines)]
fn decode_event_inner(
    config: &NatsEventStoreConfig,
    subject: &str,
    expected_stream_id: Option<&StreamId>,
    expected_last_subject_sequence: Option<u64>,
    headers: &HeaderMap,
    payload: &[u8],
) -> Result<DecodedEvent, EventStoreError> {
    if payload.len() > config.max_event_bytes() {
        return Err(corrupt("stored event exceeds the configured byte limit"));
    }
    let wire: StoredEventWire = serde_json::from_slice(payload)
        .map_err(|error| corrupt(format!("stored event is not valid wire JSON: {error}")))?;
    if !matches!(
        wire.schema_version,
        LEGACY_EVENT_SCHEMA_VERSION | CORRELATION_EVENT_SCHEMA_VERSION | EVENT_SCHEMA_VERSION
    ) {
        return Err(corrupt("stored event has an unsupported schema version"));
    }
    if wire.schema_version == LEGACY_EVENT_SCHEMA_VERSION
        && (wire.event.correlation_id.is_some() || wire.event.causation_id.is_some())
    {
        return Err(corrupt(
            "legacy stored events cannot contain correlation or causation metadata",
        ));
    }
    if wire.schema_version < EVENT_SCHEMA_VERSION
        && (wire.event.application.is_some() || wire.event.bounded_context.is_some())
    {
        return Err(corrupt(
            "legacy stored events cannot contain application scope metadata",
        ));
    }
    let expected_checksum = event_checksum(wire.schema_version, &wire.event)
        .map_err(|error| corrupt(format!("stored event cannot be checksummed: {error}")))?;
    if wire.checksum != expected_checksum {
        return Err(corrupt("stored event checksum does not match its content"));
    }
    if wire.event.event_store_stream != config.stream_name() {
        return Err(corrupt(
            "stored event belongs to a different event-store stream",
        ));
    }
    if wire.schema_version == EVENT_SCHEMA_VERSION
        && (wire.event.application.as_deref() != Some(config.application().as_str())
            || wire.event.bounded_context.as_deref() != Some(config.bounded_context().as_str()))
    {
        return Err(corrupt(
            "stored event belongs to a different application or bounded context",
        ));
    }

    let aggregate_type = AggregateType::new(wire.event.stream.aggregate_type)
        .map_err(|error| corrupt(format!("invalid stored aggregate type: {error}")))?;
    let aggregate_id = AggregateId::new(wire.event.stream.aggregate_id)
        .map_err(|error| corrupt(format!("invalid stored aggregate id: {error}")))?;
    let stream_id = StreamId::new(aggregate_type, aggregate_id);
    if expected_stream_id.is_some_and(|expected| &stream_id != expected) {
        return Err(corrupt(
            "stored event belongs to a different aggregate stream",
        ));
    }
    if config.aggregate_subject(
        stream_id.aggregate_type().as_str(),
        stream_id.aggregate_id().as_str(),
    ) != subject
    {
        return Err(corrupt("stored event is on the wrong aggregate subject"));
    }
    if wire.event.commit_event_count == 0
        || wire.event.commit_event_count as usize > MAX_EVENTS_PER_BATCH
        || wire.event.commit_event_ordinal >= wire.event.commit_event_count
    {
        return Err(corrupt("stored event has invalid commit coordinates"));
    }
    if wire.event.stream_version == 0 {
        return Err(corrupt("stored event has aggregate version zero"));
    }

    let commit_id = CommitId::new(wire.event.commit_id)
        .map_err(|error| corrupt(format!("invalid stored commit identity: {error}")))?;
    let operation_id = OperationId::new(wire.event.operation_id)
        .map_err(|error| corrupt(format!("invalid stored operation identity: {error}")))?;
    let operation_fingerprint = ContentFingerprint::from_hex(&wire.event.operation_fingerprint)
        .map_err(|error| corrupt(format!("invalid stored operation fingerprint: {error}")))?;
    let correlation_id = wire
        .event
        .correlation_id
        .map(CorrelationId::new)
        .transpose()
        .map_err(|error| corrupt(format!("invalid stored correlation identity: {error}")))?;
    let causation_id = wire
        .event
        .causation_id
        .map(CausationId::new)
        .transpose()
        .map_err(|error| corrupt(format!("invalid stored causation identity: {error}")))?;
    let event_id = EventId::new(wire.event.event_id)
        .map_err(|error| corrupt(format!("invalid stored event identity: {error}")))?;
    let payload = STANDARD
        .decode(wire.event.payload_base64)
        .map_err(|error| corrupt(format!("invalid stored event payload: {error}")))?;
    let new_event = NewEvent::new(
        event_id.clone(),
        wire.event.event_type,
        wire.event.event_schema_version,
        payload.clone(),
    )
    .map_err(|error| corrupt(format!("invalid stored event envelope: {error}")))?;
    let metadata = ExecutionMetadata::new(
        stream_id.clone(),
        operation_id.clone(),
        operation_fingerprint,
    );
    if metadata.commit_id() != &commit_id
        || metadata.event_id(wire.event.commit_event_ordinal) != event_id
    {
        return Err(corrupt("stored event has incompatible derived identities"));
    }
    let mut recorded = RecordedEvent::new_in_commit(
        stream_id,
        StreamVersion::new(wire.event.stream_version),
        event_id,
        commit_id.clone(),
        operation_id.clone(),
        operation_fingerprint,
        wire.event.commit_event_ordinal,
        wire.event.commit_event_count,
        new_event.event_type(),
        new_event.schema_version(),
        payload,
    )
    .map_err(|error| corrupt(format!("invalid recorded event: {error}")))?;
    if let Some(correlation_id) = correlation_id {
        recorded = recorded.with_correlation_id(correlation_id);
    }
    if let Some(causation_id) = causation_id {
        recorded = recorded.with_causation_id(causation_id);
    }
    let batch_id = validate_atomic_headers(
        config.stream_name(),
        headers,
        wire.event.commit_event_ordinal,
        wire.event.commit_event_count,
        expected_last_subject_sequence,
    )?;

    Ok(DecodedEvent {
        batch_id,
        commit_id,
        operation_id,
        operation_fingerprint,
        event_ordinal: wire.event.commit_event_ordinal,
        event_count: wire.event.commit_event_count,
        recorded,
    })
}

impl HistoryBuilder {
    fn push(&mut self, decoded: DecodedEvent) -> Result<(), EventStoreError> {
        let expected_version = self
            .current_version
            .next()
            .ok_or_else(|| corrupt("aggregate version space overflowed"))?;
        if decoded.recorded.stream_version() != expected_version {
            return Err(corrupt(
                "aggregate events are missing, duplicated, or noncontiguous",
            ));
        }
        if !self.event_ids.insert(decoded.recorded.event_id().clone()) {
            return Err(corrupt(
                "aggregate history contains a duplicate event identity",
            ));
        }

        if decoded.event_ordinal == 0 {
            if self.pending.is_some() {
                return Err(corrupt("aggregate history contains an incomplete commit"));
            }
            if !self.operation_ids.insert(decoded.operation_id.clone()) {
                return Err(corrupt(
                    "aggregate history contains a duplicate operation identity",
                ));
            }
            if !self.commit_ids.insert(decoded.commit_id.clone()) {
                return Err(corrupt(
                    "aggregate history contains a duplicate commit identity",
                ));
            }
            self.pending = Some(PendingCommit::new(decoded));
        } else {
            self.pending
                .as_mut()
                .ok_or_else(|| corrupt("aggregate history starts inside a commit"))?
                .push(decoded)?;
        }

        self.current_version = expected_version;
        if self
            .pending
            .as_ref()
            .is_some_and(PendingCommit::is_complete)
        {
            let stored = self
                .pending
                .take()
                .expect("completed pending commit exists")
                .finish()?;
            self.history.events.extend(stored.events.iter().cloned());
            self.history.commits.push(stored);
        }
        Ok(())
    }

    fn finish(mut self, last_sequence: u64) -> Result<History, EventStoreError> {
        if self.pending.is_some() || self.current_version == StreamVersion::ZERO {
            return Err(corrupt("aggregate history ended inside a commit"));
        }
        self.history.last_subject_stream_sequence = last_sequence;
        Ok(self.history)
    }
}

impl PendingCommit {
    fn new(decoded: DecodedEvent) -> Self {
        Self {
            batch_id: decoded.batch_id,
            commit_id: decoded.commit_id,
            operation_id: decoded.operation_id,
            operation_fingerprint: decoded.operation_fingerprint,
            event_count: decoded.event_count,
            events: vec![decoded.recorded],
        }
    }

    fn push(&mut self, decoded: DecodedEvent) -> Result<(), EventStoreError> {
        let expected_ordinal = u32::try_from(self.events.len())
            .map_err(|_| corrupt("stored event ordinal cannot be represented"))?;
        if decoded.event_ordinal != expected_ordinal
            || decoded.event_count != self.event_count
            || decoded.batch_id != self.batch_id
            || decoded.commit_id != self.commit_id
            || decoded.operation_id != self.operation_id
            || decoded.operation_fingerprint != self.operation_fingerprint
            || decoded.recorded.correlation_id() != self.events[0].correlation_id()
            || decoded.recorded.causation_id() != self.events[0].causation_id()
        {
            return Err(corrupt("stored commit metadata is inconsistent"));
        }
        self.events.push(decoded.recorded);
        Ok(())
    }

    fn is_complete(&self) -> bool {
        self.events.len() == self.event_count as usize
    }

    fn finish(self) -> Result<StoredCommit, EventStoreError> {
        let new_events = self
            .events
            .iter()
            .map(|event| {
                NewEvent::new(
                    event.event_id().clone(),
                    event.event_type(),
                    event.schema_version(),
                    event.payload().to_vec(),
                )
                .map_err(|error| corrupt(format!("invalid stored event envelope: {error}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut batch = EventBatch::new(
            self.commit_id,
            self.operation_id,
            self.operation_fingerprint,
            new_events,
        )
        .map_err(|error| corrupt(format!("invalid stored event batch: {error}")))?;
        if let Some(correlation_id) = self.events[0].correlation_id() {
            batch = batch.with_correlation_id(correlation_id.clone());
        }
        if let Some(causation_id) = self.events[0].causation_id() {
            batch = batch.with_causation_id(causation_id.clone());
        }
        Ok(StoredCommit {
            batch,
            events: self.events,
        })
    }
}

#[derive(Deserialize)]
struct AtomicPublishAck {
    stream: String,
    #[serde(rename = "seq")]
    sequence: u64,
    #[serde(default)]
    batch: Option<String>,
    #[serde(default)]
    count: Option<u64>,
}

enum AtomicBatchPublishError {
    Expectation,
    Store(EventStoreError),
}

async fn publish_atomic_batch(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
    subject: &str,
    expected_last_subject_sequence: u64,
    batch_id: &str,
    payloads: Vec<Vec<u8>>,
) -> Result<AtomicPublishAck, AtomicBatchPublishError> {
    let event_count = payloads.len();
    for (index, payload) in payloads.into_iter().enumerate() {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert(NATS_REQUIRED_API_LEVEL, ATOMIC_BATCH_API_LEVEL);
        headers.insert(NATS_BATCH_ID, batch_id);
        headers.insert(NATS_BATCH_SEQUENCE, (index + 1).to_string());
        if index == 0 {
            headers.insert(NATS_EXPECTED_STREAM, config.stream_name());
            headers.insert(
                NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                expected_last_subject_sequence.to_string(),
            );
        }
        if index + 1 == event_count {
            headers.insert(NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL);
        }

        let message = context
            .client()
            .send_request(
                subject.to_owned(),
                Request::new()
                    .headers(headers)
                    .payload(payload.into())
                    .timeout(Some(config.puback_timeout())),
            )
            .await
            .map_err(|error| {
                AtomicBatchPublishError::Store(unavailable(format!(
                    "atomic event publish failed: {error}"
                )))
            })?;

        if index + 1 != event_count {
            if message.payload.is_empty() {
                continue;
            }
            return match decode_atomic_publish_response(message.payload.as_ref()) {
                Ok(_) => Err(AtomicBatchPublishError::Store(corrupt(
                    "NATS acknowledged an atomic batch before its final event",
                ))),
                Err(error) => Err(error),
            };
        }
        if message.payload.is_empty() {
            return Err(AtomicBatchPublishError::Store(corrupt(
                "NATS omitted the atomic batch PubAck",
            )));
        }
        return decode_atomic_publish_response(message.payload.as_ref());
    }
    Err(AtomicBatchPublishError::Store(invalid(
        "cannot publish an empty atomic batch",
    )))
}

fn decode_atomic_publish_response(
    payload: &[u8],
) -> Result<AtomicPublishAck, AtomicBatchPublishError> {
    let response: Response<AtomicPublishAck> =
        serde_json::from_slice(payload).map_err(|error| {
            AtomicBatchPublishError::Store(corrupt(format!(
                "NATS returned an invalid atomic PubAck: {error}"
            )))
        })?;
    match response {
        Response::Ok(ack) => Ok(ack),
        Response::Err { error } => Err(classify_atomic_api_error(&error)),
    }
}

fn classify_atomic_api_error(error: &jetstream::Error) -> AtomicBatchPublishError {
    let code = error.error_code();
    if code == jetstream::ErrorCode::STREAM_WRONG_LAST_SEQUENCE
        || code == jetstream::ErrorCode::STREAM_SEQUENCE_NOT_MATCH
    {
        AtomicBatchPublishError::Expectation
    } else if code == jetstream::ErrorCode::STREAM_STORE_FAILED
        && error.to_string().starts_with("maximum bytes exceeded (")
    {
        AtomicBatchPublishError::Store(EventStoreError::new(
            EventStoreErrorKind::CapacityExhausted,
            "configured event-store byte capacity is exhausted",
        ))
    } else if code == jetstream::ErrorCode::ATOMIC_PUBLISH_DISABLED
        || code == jetstream::ErrorCode::REQUIRED_API_LEVEL
    {
        AtomicBatchPublishError::Store(EventStoreError::new(
            EventStoreErrorKind::ConfigurationMismatch,
            format!("event-store stream does not support atomic publishing: {error}"),
        ))
    } else {
        AtomicBatchPublishError::Store(unavailable(format!(
            "atomic event publish was rejected: {error}"
        )))
    }
}

fn new_atomic_batch_id(client: &async_nats::Client, commit_id: &CommitId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(client.new_inbox().as_bytes());
    hasher.update(commit_id.as_str().as_bytes());
    let mut output = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn validate_atomic_headers(
    stream_name: &str,
    headers: &HeaderMap,
    event_ordinal: u32,
    event_count: u32,
    expected_last_subject_sequence: Option<u64>,
) -> Result<String, EventStoreError> {
    if required_single_header(headers, "Content-Type")? != "application/json" {
        return Err(corrupt("stored event has the wrong content type"));
    }
    let batch_id = required_single_header(headers, "Nats-Batch-Id")?;
    if batch_id.is_empty() || batch_id.len() > 64 {
        return Err(corrupt("stored event has an invalid atomic batch identity"));
    }
    let batch_sequence = required_single_header(headers, "Nats-Batch-Sequence")?
        .parse::<u32>()
        .map_err(|_| corrupt("stored event has an invalid atomic batch sequence"))?;
    if batch_sequence != event_ordinal + 1 {
        return Err(corrupt(
            "stored event atomic batch sequence does not match its commit ordinal",
        ));
    }
    let expected_stream = optional_single_header(headers, "Nats-Expected-Stream")?;
    let expected_sequence = optional_single_header(headers, "Nats-Expected-Last-Subject-Sequence")?;
    if event_ordinal == 0 {
        if expected_stream != Some(stream_name) {
            return Err(corrupt("stored commit has an incompatible expected stream"));
        }
        let sequence = expected_sequence
            .ok_or_else(|| corrupt("stored commit has no aggregate sequence expectation"))?
            .parse::<u64>()
            .map_err(|_| corrupt("stored commit has an invalid aggregate sequence expectation"))?;
        if expected_last_subject_sequence.is_some_and(|expected| sequence != expected) {
            return Err(corrupt(
                "stored commit has an incompatible aggregate sequence expectation",
            ));
        }
    } else if expected_stream.is_some() || expected_sequence.is_some() {
        return Err(corrupt(
            "stored commit repeats its aggregate sequence expectation",
        ));
    }
    let commit = optional_single_header(headers, "Nats-Batch-Commit")?;
    if event_ordinal + 1 == event_count {
        if commit != Some(NATS_BATCH_COMMIT_FINAL) {
            return Err(corrupt("stored commit has no atomic final event"));
        }
    } else if commit.is_some() {
        return Err(corrupt("stored commit was finalized before its last event"));
    }
    Ok(batch_id.to_owned())
}

fn required_single_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<&'a str, EventStoreError> {
    optional_single_header(headers, name)?
        .ok_or_else(|| corrupt(format!("stored event is missing {name}")))
}

fn optional_single_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a str>, EventStoreError> {
    let mut values = headers.get_all(name);
    let value = values.next().map(async_nats::HeaderValue::as_str);
    if values.next().is_some() {
        return Err(corrupt(format!("stored event repeats {name}")));
    }
    Ok(value)
}

fn record_batch(
    stream_id: &StreamId,
    current_version: StreamVersion,
    batch: &EventBatch,
) -> Result<Vec<RecordedEvent>, EventStoreError> {
    let mut version = current_version;
    let mut recorded = Vec::with_capacity(batch.events().len());
    let event_count = u32::try_from(batch.events().len())
        .map_err(|_| invalid("event count exceeds the supported range"))?;
    for (ordinal, event) in batch.events().iter().enumerate() {
        version = version.next().ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::CapacityExhausted,
                "aggregate version space is exhausted",
            )
        })?;
        let mut recorded_event = RecordedEvent::new_in_commit(
            stream_id.clone(),
            version,
            event.event_id().clone(),
            batch.commit_id().clone(),
            batch.operation_id().clone(),
            batch.operation_fingerprint(),
            u32::try_from(ordinal)
                .map_err(|_| invalid("event ordinal exceeds the supported range"))?,
            event_count,
            event.event_type(),
            event.schema_version(),
            event.payload().to_vec(),
        )
        .map_err(|error| invalid(format!("invalid event envelope: {error}")))?;
        if let Some(correlation_id) = batch.correlation_id() {
            recorded_event = recorded_event.with_correlation_id(correlation_id.clone());
        }
        if let Some(causation_id) = batch.causation_id() {
            recorded_event = recorded_event.with_causation_id(causation_id.clone());
        }
        recorded.push(recorded_event);
    }
    Ok(recorded)
}

fn same_batch(stored: &EventBatch, incoming: &EventBatch) -> bool {
    stored.commit_id() == incoming.commit_id()
        && stored.operation_id() == incoming.operation_id()
        && stored.operation_fingerprint() == incoming.operation_fingerprint()
        && stored.correlation_id() == incoming.correlation_id()
        && stored.causation_id() == incoming.causation_id()
        && stored.events() == incoming.events()
}

fn validate_derived_identities(
    stream_id: &StreamId,
    batch: &EventBatch,
) -> Result<(), EventStoreError> {
    let metadata = ExecutionMetadata::new(
        stream_id.clone(),
        batch.operation_id().clone(),
        batch.operation_fingerprint(),
    );
    if batch.commit_id() != metadata.commit_id() {
        return Err(invalid(
            "commit identity was not derived from the stream and operation identity",
        ));
    }
    for (ordinal, event) in batch.events().iter().enumerate() {
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| invalid("event ordinal exceeds the supported range"))?;
        if event.event_id() != &metadata.event_id(ordinal) {
            return Err(invalid(
                "event identity was not derived from its commit identity and ordinal",
            ));
        }
    }
    Ok(())
}

fn event_checksum(
    schema_version: u16,
    event: &StoredEventContentWire,
) -> Result<String, serde_json::Error> {
    let input = serde_json::to_vec(&ChecksumInput {
        schema_version,
        event,
    })?;
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(input) {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(output)
}

fn verify_stream_config(expected: &Config, actual: &Config) -> Result<(), EventStoreError> {
    let mismatches = stream_config_mismatches(expected, actual);
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(EventStoreError::new(
            EventStoreErrorKind::ConfigurationMismatch,
            format!(
                "existing JetStream configuration differs in: {}",
                mismatches.join(", ")
            ),
        ))
    }
}

fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}

fn conflict(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::Conflict, message)
}

fn identity_conflict(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::IdentityConflict, message)
}

fn corrupt(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::CorruptHistory, message)
}

fn unavailable(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::Unavailable, message)
}

#[cfg(test)]
mod tests {
    use rostfrei_messaging_core::ApplicationName;

    use super::*;

    fn config() -> NatsEventStoreConfig {
        let context = ApplicationName::new("acme")
            .unwrap()
            .bounded_context("orders")
            .unwrap();
        NatsEventStoreConfig::new(&context, "EVENTS").unwrap()
    }

    fn stream_id() -> StreamId {
        StreamId::new(
            AggregateType::new("Test").unwrap(),
            AggregateId::new("one").unwrap(),
        )
    }

    fn atomic_headers(stream_name: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert(NATS_BATCH_ID, "schema-compatibility");
        headers.insert(NATS_BATCH_SEQUENCE, "1");
        headers.insert(NATS_EXPECTED_STREAM, stream_name);
        headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, "0");
        headers.insert(NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL);
        headers
    }

    #[test]
    fn legacy_event_checksum_remains_stable_when_metadata_is_absent() {
        let legacy = br#"{
            "schemaVersion": 1,
            "checksum": "024a76677649a206da30ac19e3bededf39a4666cfd9ab39a9ae4b1a9280e52db",
            "event": {
                "eventStoreStream": "EVENTS",
                "stream": {"aggregateType": "Test", "aggregateId": "one"},
                "streamVersion": 1,
                "commitId": "commit-1",
                "operationId": "operation-1",
                "operationFingerprint": "0000000000000000000000000000000000000000000000000000000000000000",
                "commitEventOrdinal": 0,
                "commitEventCount": 1,
                "eventId": "event-1",
                "eventType": "opened",
                "eventSchemaVersion": 1,
                "payloadBase64": "e30="
            }
        }"#;

        let wire: StoredEventWire = serde_json::from_slice(legacy).expect("legacy event wire");

        assert_eq!(wire.schema_version, LEGACY_EVENT_SCHEMA_VERSION);
        assert_eq!(wire.event.correlation_id, None);
        assert_eq!(wire.event.causation_id, None);
        assert_eq!(wire.event.application, None);
        assert_eq!(wire.event.bounded_context, None);
        assert_eq!(
            event_checksum(wire.schema_version, &wire.event).expect("legacy checksum"),
            wire.checksum
        );
        let reencoded = serde_json::to_value(&wire.event).expect("legacy event content");
        assert!(reencoded.get("correlationId").is_none());
        assert!(reencoded.get("causationId").is_none());
        assert!(reencoded.get("application").is_none());
        assert!(reencoded.get("boundedContext").is_none());
    }

    #[test]
    fn schema_two_fixture_preserves_checksum_and_exact_replay() {
        let fixture = br#"{
            "schemaVersion": 2,
            "checksum": "500416c7d826fb8ee897d371d1269c2b4aa6437ec14817c78ce1d4f04616d072",
            "event": {
                "eventStoreStream": "EVENTS",
                "stream": {"aggregateType": "Test", "aggregateId": "one"},
                "streamVersion": 1,
                "commitId": "commit:058e7cef4bd684348e646c0abcceef8a2e3bb6ba386c73e4931c77693ecded54",
                "operationId": "schema-2-operation",
                "operationFingerprint": "fee21a1bfc244307e772580bf87fa36197609b1a31de0e9b133b1d73282aba4d",
                "correlationId": "correlation-1",
                "causationId": "causation-1",
                "commitEventOrdinal": 0,
                "commitEventCount": 1,
                "eventId": "event:3219be45a3c44e07a9295a7b2044c376ccea46a791c804e57293a18d405afa92",
                "eventType": "opened",
                "eventSchemaVersion": 1,
                "payloadBase64": "e30="
            }
        }"#;
        let wire: StoredEventWire = serde_json::from_slice(fixture).expect("schema-2 fixture");
        assert_eq!(wire.schema_version, CORRELATION_EVENT_SCHEMA_VERSION);
        assert_eq!(
            event_checksum(wire.schema_version, &wire.event).expect("schema-2 checksum"),
            wire.checksum
        );

        let config = config();
        let stream_id = stream_id();
        let subject = config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let decoded = decode_event(
            &config,
            &subject,
            &stream_id,
            Some(0),
            &atomic_headers(config.stream_name()),
            fixture,
        )
        .expect("schema-2 decode");
        let mut builder = HistoryBuilder::default();
        builder.push(decoded).expect("schema-2 history event");
        let history = builder.finish(1).expect("schema-2 history");

        let operation_id = OperationId::new("schema-2-operation").unwrap();
        let fingerprint = ContentFingerprint::from_hex(
            "fee21a1bfc244307e772580bf87fa36197609b1a31de0e9b133b1d73282aba4d",
        )
        .unwrap();
        let metadata = ExecutionMetadata::new(stream_id, operation_id.clone(), fingerprint);
        let event = NewEvent::new(metadata.event_id(0), "opened", 1, b"{}".to_vec()).unwrap();
        let batch = EventBatch::new(
            metadata.commit_id().clone(),
            operation_id,
            fingerprint,
            vec![event.clone()],
        )
        .unwrap()
        .with_correlation_id(CorrelationId::new("correlation-1").unwrap())
        .with_causation_id(CausationId::new("causation-1").unwrap());

        assert_eq!(
            NatsEventStore::resolve_existing(&history, &batch).expect("exact replay"),
            Some(history.events.clone())
        );
        let changed = EventBatch::new(
            metadata.commit_id().clone(),
            metadata.operation_id().clone(),
            fingerprint,
            vec![event],
        )
        .unwrap()
        .with_correlation_id(CorrelationId::new("correlation-2").unwrap())
        .with_causation_id(CausationId::new("causation-1").unwrap());
        assert_eq!(
            NatsEventStore::resolve_existing(&history, &changed)
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::IdentityConflict
        );
    }

    #[test]
    fn schema_three_scope_is_required_and_checked_after_checksum_validation() {
        let config = config();
        let stream_id = stream_id();
        let operation_id = OperationId::new("schema-3-operation").unwrap();
        let fingerprint = ContentFingerprint::digest("schema-3-content");
        let metadata = ExecutionMetadata::new(stream_id.clone(), operation_id.clone(), fingerprint);
        let event = NewEvent::new(metadata.event_id(0), "opened", 1, b"{}".to_vec()).unwrap();
        let batch = EventBatch::new(
            metadata.commit_id().clone(),
            operation_id,
            fingerprint,
            vec![event],
        )
        .unwrap();
        let recorded = record_batch(&stream_id, StreamVersion::ZERO, &batch).unwrap();
        let payload = encode_events(&config, &stream_id, &batch, &recorded)
            .unwrap()
            .remove(0);
        let subject = config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let headers = atomic_headers(config.stream_name());
        decode_event(&config, &subject, &stream_id, Some(0), &headers, &payload)
            .expect("schema-3 decode");
        let wire: StoredEventWire = serde_json::from_slice(&payload).unwrap();

        for (application, bounded_context) in [
            (Some("other"), Some("orders")),
            (Some("acme"), Some("billing")),
            (None, Some("orders")),
            (Some("acme"), None),
        ] {
            let mut changed = wire.clone();
            changed.event.application = application.map(str::to_owned);
            changed.event.bounded_context = bounded_context.map(str::to_owned);
            changed.checksum = event_checksum(changed.schema_version, &changed.event).unwrap();
            let payload = serde_json::to_vec(&changed).unwrap();

            let result = decode_event(&config, &subject, &stream_id, Some(0), &headers, &payload);
            assert!(matches!(
                result,
                Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory
            ));
        }
    }
}
