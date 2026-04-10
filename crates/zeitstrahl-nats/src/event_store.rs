use std::collections::HashSet;
use std::error::Error as _;
use std::fmt::Write as _;

use async_nats::jetstream::{
    self,
    context::{PublishError, PublishErrorKind},
    message::PublishMessage,
    stream::{Config, DiscardPolicy, LastRawMessageErrorKind, RetentionPolicy, StorageType},
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeitstrahl_core::{
    AggregateId, AggregateType, AppendOutcome, CommitId, ContentFingerprint, EventBatch, EventId,
    EventStore, EventStoreError, EventStoreErrorKind, ExecutionMetadata, ExpectedVersion, NewEvent,
    OperationId, RecordedEvent, StreamId, StreamVersion, MAX_EVENTS_PER_BATCH,
};

use crate::event_store_config::NatsEventStoreConfig;

const COMMIT_SCHEMA_VERSION: u16 = 1;

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

    #[allow(clippy::too_many_lines)]
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

        let mut history = History {
            last_subject_stream_sequence: last_sequence,
            ..History::default()
        };
        let mut next_stream_sequence = 1_u64;
        let mut current_version = StreamVersion::ZERO;
        let mut operation_ids = HashSet::new();
        let mut commit_ids = HashSet::new();
        let mut event_ids = HashSet::new();

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

            let decoded =
                decode_commit(&self.config, &subject, stream_id, message.payload.as_ref())?;
            let expected_first = current_version
                .next()
                .ok_or_else(|| corrupt("aggregate version space overflowed"))?;
            if decoded.first_version != expected_first {
                return Err(corrupt(
                    "aggregate commits are missing, duplicated, or noncontiguous",
                ));
            }
            if !operation_ids.insert(decoded.batch.operation_id().clone()) {
                return Err(corrupt(
                    "aggregate history contains a duplicate operation identity",
                ));
            }
            if !commit_ids.insert(decoded.batch.commit_id().clone()) {
                return Err(corrupt(
                    "aggregate history contains a duplicate commit identity",
                ));
            }
            for event in &decoded.events {
                if !event_ids.insert(event.event_id().clone()) {
                    return Err(corrupt(
                        "aggregate history contains a duplicate event identity",
                    ));
                }
            }

            current_version = decoded.last_version;
            history.events.extend(decoded.events.iter().cloned());
            history.commits.push(StoredCommit {
                batch: decoded.batch,
                events: decoded.events,
            });

            if message.sequence == last_sequence {
                break;
            }
            next_stream_sequence = message
                .sequence
                .checked_add(1)
                .ok_or_else(|| corrupt("JetStream sequence space overflowed"))?;
        }

        if next_stream_sequence > last_sequence
            || history
                .commits
                .last()
                .is_none_or(|_| current_version == StreamVersion::ZERO)
        {
            return Err(corrupt("aggregate history ended before its last message"));
        }
        Ok(history)
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
        expected_events: &[RecordedEvent],
    ) -> Result<(), EventStoreError> {
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
        let decoded = decode_commit(&self.config, subject, stream_id, message.payload.as_ref())?;
        if decoded.events != expected_events {
            return Err(corrupt("PubAck sequence contains a different commit"));
        }
        Ok(())
    }
}

#[async_trait]
impl EventStore for NatsEventStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        Ok(self.load_history(stream_id).await?.events)
    }

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
        let payload = encode_commit(&self.config, stream_id, &batch, &recorded)?;
        if payload.len() > self.config.max_commit_bytes() {
            return Err(invalid(format!(
                "encoded commit exceeds the configured {}-byte limit",
                self.config.max_commit_bytes()
            )));
        }

        let subject = self.config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let message = PublishMessage::build()
            .payload(payload.into())
            .header("Content-Type", "application/json")
            .message_id(batch.commit_id().as_str())
            .expected_stream(self.config.stream_name())
            .expected_last_subject_sequence(history.last_subject_stream_sequence);
        let mut context = self.context.clone();
        context.set_timeout(self.config.puback_timeout());
        let ack_future = match context.send_publish(subject.clone(), message).await {
            Ok(future) => future,
            Err(error) if is_expectation_error(&error) => {
                return self.resolve_expectation_race(stream_id, &batch).await;
            }
            Err(error) => return Err(classify_publish_error(&error)),
        };
        let ack = match ack_future.await {
            Ok(ack) => ack,
            Err(error) if is_expectation_error(&error) => {
                return self.resolve_expectation_race(stream_id, &batch).await;
            }
            Err(error) => return Err(classify_publish_error(&error)),
        };
        if ack.stream != self.config.stream_name()
            || ack.sequence == 0
            || ack.sequence <= history.last_subject_stream_sequence
        {
            return Err(corrupt(
                "PubAck returned an incompatible stream or sequence",
            ));
        }
        if ack.duplicate {
            return self.resolve_expectation_race(stream_id, &batch).await;
        }
        self.verify_published_commit(stream_id, &subject, ack.sequence, &recorded)
            .await?;
        Ok(AppendOutcome::Appended(recorded))
    }
}

pub async fn provision_event_store(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
) -> Result<(), EventStoreError> {
    context
        .create_or_update_stream(config.stream_config())
        .await
        .map_err(|error| unavailable(format!("failed to provision event-store stream: {error}")))?;
    Ok(())
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

struct DecodedCommit {
    first_version: StreamVersion,
    last_version: StreamVersion,
    batch: EventBatch,
    events: Vec<RecordedEvent>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommitWire {
    schema_version: u16,
    checksum: String,
    commit: CommitContentWire,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommitContentWire {
    event_store_stream: String,
    stream: StreamIdentityWire,
    first_version: u64,
    last_version: u64,
    commit_id: String,
    operation_id: String,
    operation_fingerprint: String,
    events: Vec<EventWire>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StreamIdentityWire {
    aggregate_type: String,
    aggregate_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventWire {
    event_id: String,
    event_type: String,
    schema_version: u32,
    payload_base64: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChecksumInput<'a> {
    schema_version: u16,
    commit: &'a CommitContentWire,
}

fn encode_commit(
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    batch: &EventBatch,
    recorded: &[RecordedEvent],
) -> Result<Vec<u8>, EventStoreError> {
    let first_version = recorded
        .first()
        .ok_or_else(|| invalid("cannot encode an empty commit"))?
        .stream_version()
        .value();
    let last_version = recorded
        .last()
        .ok_or_else(|| invalid("cannot encode an empty commit"))?
        .stream_version()
        .value();
    let commit = CommitContentWire {
        event_store_stream: config.stream_name().to_owned(),
        stream: StreamIdentityWire {
            aggregate_type: stream_id.aggregate_type().as_str().to_owned(),
            aggregate_id: stream_id.aggregate_id().as_str().to_owned(),
        },
        first_version,
        last_version,
        commit_id: batch.commit_id().as_str().to_owned(),
        operation_id: batch.operation_id().as_str().to_owned(),
        operation_fingerprint: batch.operation_fingerprint().to_hex(),
        events: batch
            .events()
            .iter()
            .map(|event| EventWire {
                event_id: event.event_id().as_str().to_owned(),
                event_type: event.event_type().to_owned(),
                schema_version: event.schema_version(),
                payload_base64: STANDARD.encode(event.payload()),
            })
            .collect(),
    };
    let checksum = commit_checksum(&commit)
        .map_err(|error| invalid(format!("failed to checksum commit: {error}")))?;
    serde_json::to_vec(&CommitWire {
        schema_version: COMMIT_SCHEMA_VERSION,
        checksum,
        commit,
    })
    .map_err(|error| invalid(format!("failed to encode commit: {error}")))
}

#[allow(clippy::too_many_lines)]
fn decode_commit(
    config: &NatsEventStoreConfig,
    subject: &str,
    expected_stream_id: &StreamId,
    payload: &[u8],
) -> Result<DecodedCommit, EventStoreError> {
    if payload.len() > config.max_commit_bytes() {
        return Err(corrupt("stored commit exceeds the configured byte limit"));
    }
    let wire: CommitWire = serde_json::from_slice(payload)
        .map_err(|error| corrupt(format!("stored commit is not valid wire JSON: {error}")))?;
    if wire.schema_version != COMMIT_SCHEMA_VERSION {
        return Err(corrupt("stored commit has an unsupported schema version"));
    }
    let expected_checksum = commit_checksum(&wire.commit)
        .map_err(|error| corrupt(format!("stored commit cannot be checksummed: {error}")))?;
    if wire.checksum != expected_checksum {
        return Err(corrupt("stored commit checksum does not match its content"));
    }
    if wire.commit.event_store_stream != config.stream_name() {
        return Err(corrupt(
            "stored commit belongs to a different event-store stream",
        ));
    }

    let aggregate_type = AggregateType::new(wire.commit.stream.aggregate_type)
        .map_err(|error| corrupt(format!("invalid stored aggregate type: {error}")))?;
    let aggregate_id = AggregateId::new(wire.commit.stream.aggregate_id)
        .map_err(|error| corrupt(format!("invalid stored aggregate id: {error}")))?;
    let stream_id = StreamId::new(aggregate_type, aggregate_id);
    if &stream_id != expected_stream_id {
        return Err(corrupt(
            "stored commit belongs to a different aggregate stream",
        ));
    }
    if config.aggregate_subject(
        stream_id.aggregate_type().as_str(),
        stream_id.aggregate_id().as_str(),
    ) != subject
    {
        return Err(corrupt("stored commit is on the wrong aggregate subject"));
    }
    if wire.commit.events.is_empty() || wire.commit.events.len() > MAX_EVENTS_PER_BATCH {
        return Err(corrupt("stored commit has an invalid event count"));
    }
    if wire.commit.first_version == 0 {
        return Err(corrupt("stored commit starts at aggregate version zero"));
    }
    let calculated_last = wire
        .commit
        .first_version
        .checked_add(
            u64::try_from(wire.commit.events.len() - 1)
                .map_err(|_| corrupt("stored commit event count cannot be represented"))?,
        )
        .ok_or_else(|| corrupt("stored commit version range overflowed"))?;
    if wire.commit.last_version != calculated_last {
        return Err(corrupt("stored commit has a noncontiguous version range"));
    }

    let commit_id = CommitId::new(wire.commit.commit_id)
        .map_err(|error| corrupt(format!("invalid stored commit identity: {error}")))?;
    let operation_id = OperationId::new(wire.commit.operation_id)
        .map_err(|error| corrupt(format!("invalid stored operation identity: {error}")))?;
    let operation_fingerprint = ContentFingerprint::from_hex(&wire.commit.operation_fingerprint)
        .map_err(|error| corrupt(format!("invalid stored operation fingerprint: {error}")))?;
    let new_events = wire
        .commit
        .events
        .into_iter()
        .map(|event| {
            let event_id = EventId::new(event.event_id)
                .map_err(|error| corrupt(format!("invalid stored event identity: {error}")))?;
            let payload = STANDARD
                .decode(event.payload_base64)
                .map_err(|error| corrupt(format!("invalid stored event payload: {error}")))?;
            NewEvent::new(event_id, event.event_type, event.schema_version, payload)
                .map_err(|error| corrupt(format!("invalid stored event envelope: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let batch = EventBatch::new(commit_id, operation_id, operation_fingerprint, new_events)
        .map_err(|error| corrupt(format!("invalid stored event batch: {error}")))?;
    validate_derived_identities(&stream_id, &batch)
        .map_err(|error| corrupt(format!("incompatible stored identities: {error}")))?;

    let first_version = StreamVersion::new(wire.commit.first_version);
    let last_version = StreamVersion::new(wire.commit.last_version);
    let mut version = first_version;
    let mut events = Vec::with_capacity(batch.events().len());
    for (index, event) in batch.events().iter().enumerate() {
        if index != 0 {
            version = version
                .next()
                .ok_or_else(|| corrupt("stored event version overflowed"))?;
        }
        events.push(
            RecordedEvent::new(
                stream_id.clone(),
                version,
                event.event_id().clone(),
                batch.commit_id().clone(),
                batch.operation_id().clone(),
                batch.operation_fingerprint(),
                event.event_type(),
                event.schema_version(),
                event.payload().to_vec(),
            )
            .map_err(|error| corrupt(format!("invalid recorded event: {error}")))?,
        );
    }
    if events.last().map(RecordedEvent::stream_version) != Some(last_version) {
        return Err(corrupt(
            "stored events do not fill their commit version range",
        ));
    }
    Ok(DecodedCommit {
        first_version,
        last_version,
        batch,
        events,
    })
}

fn record_batch(
    stream_id: &StreamId,
    current_version: StreamVersion,
    batch: &EventBatch,
) -> Result<Vec<RecordedEvent>, EventStoreError> {
    let mut version = current_version;
    let mut recorded = Vec::with_capacity(batch.events().len());
    for event in batch.events() {
        version = version.next().ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::CapacityExhausted,
                "aggregate version space is exhausted",
            )
        })?;
        recorded.push(
            RecordedEvent::new(
                stream_id.clone(),
                version,
                event.event_id().clone(),
                batch.commit_id().clone(),
                batch.operation_id().clone(),
                batch.operation_fingerprint(),
                event.event_type(),
                event.schema_version(),
                event.payload().to_vec(),
            )
            .map_err(|error| invalid(format!("invalid event envelope: {error}")))?,
        );
    }
    Ok(recorded)
}

fn same_batch(stored: &EventBatch, incoming: &EventBatch) -> bool {
    stored.commit_id() == incoming.commit_id()
        && stored.operation_id() == incoming.operation_id()
        && stored.operation_fingerprint() == incoming.operation_fingerprint()
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

fn commit_checksum(commit: &CommitContentWire) -> Result<String, serde_json::Error> {
    let input = serde_json::to_vec(&ChecksumInput {
        schema_version: COMMIT_SCHEMA_VERSION,
        commit,
    })?;
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(input) {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(output)
}

fn verify_stream_config(expected: &Config, actual: &Config) -> Result<(), EventStoreError> {
    let mut mismatches = Vec::new();
    if actual.name != expected.name {
        mismatches.push("name");
    }
    if actual.subjects != expected.subjects {
        mismatches.push("subjects");
    }
    if actual.retention != RetentionPolicy::Limits {
        mismatches.push("retention");
    }
    if actual.storage != StorageType::File {
        mismatches.push("storage");
    }
    if actual.discard != DiscardPolicy::New {
        mismatches.push("discard");
    }
    if actual.discard_new_per_subject {
        mismatches.push("discard_new_per_subject");
    }
    if !actual.max_age.is_zero() {
        mismatches.push("max_age");
    }
    if actual.max_messages != -1 {
        mismatches.push("max_messages");
    }
    if actual.max_messages_per_subject != -1 {
        mismatches.push("max_messages_per_subject");
    }
    if actual.max_bytes != expected.max_bytes {
        mismatches.push("max_bytes");
    }
    if actual.max_message_size != expected.max_message_size {
        mismatches.push("max_message_size");
    }
    if actual.max_consumers != -1 {
        mismatches.push("max_consumers");
    }
    if actual.no_ack {
        mismatches.push("no_ack");
    }
    if actual.duplicate_window != expected.duplicate_window {
        mismatches.push("duplicate_window");
    }
    if actual.num_replicas != expected.num_replicas {
        mismatches.push("num_replicas");
    }
    if !actual.deny_delete {
        mismatches.push("deny_delete");
    }
    if !actual.deny_purge {
        mismatches.push("deny_purge");
    }
    if actual.allow_rollup {
        mismatches.push("allow_rollup");
    }
    if actual.sealed {
        mismatches.push("sealed");
    }
    if !actual.template_owner.is_empty() {
        mismatches.push("template_owner");
    }
    if actual.republish.is_some() {
        mismatches.push("republish");
    }
    if actual.mirror.is_some() {
        mismatches.push("mirror");
    }
    if actual.sources.is_some() {
        mismatches.push("sources");
    }
    if actual.subject_transform.is_some() {
        mismatches.push("subject_transform");
    }
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

fn classify_publish_error(error: &PublishError) -> EventStoreError {
    if is_configured_capacity_error(error) {
        EventStoreError::new(
            EventStoreErrorKind::CapacityExhausted,
            "configured event-store byte capacity is exhausted",
        )
    } else {
        unavailable(format!("event-store publish failed: {error}"))
    }
}

fn is_expectation_error(error: &PublishError) -> bool {
    error.kind() == PublishErrorKind::WrongLastSequence
        || jetstream_api_error(error).is_some_and(|error| {
            error.error_code() == jetstream::ErrorCode::STREAM_SEQUENCE_NOT_MATCH
        })
}

fn is_configured_capacity_error(error: &PublishError) -> bool {
    jetstream_api_error(error).is_some_and(|error| {
        error.error_code() == jetstream::ErrorCode::STREAM_STORE_FAILED
            && error.to_string().starts_with("maximum bytes exceeded (")
    })
}

fn jetstream_api_error(error: &PublishError) -> Option<&jetstream::Error> {
    let mut source = error.source();
    while let Some(current) = source {
        if let Some(error) = current.downcast_ref::<jetstream::Error>() {
            return Some(error);
        }
        source = current.source();
    }
    None
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
