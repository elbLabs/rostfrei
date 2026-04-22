use std::collections::HashSet;

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
    EventHistory, EventId, EventStore, EventStoreError, EventStoreErrorKind, EventTransaction,
    ExecutionMetadata, ExpectedVersion, MAX_EVENTS_PER_BATCH, NewEvent, OperationId, RecordedEvent,
    StreamId, StreamVersion, TransactionAppendOutcome, TransactionReceipt,
    TransactionStreamReceipt, validate_transaction_item_limit,
};
use rostfrei_messaging_core::{CausationId, CorrelationId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::event_store_config::{LEGACY_EVENT_STORE_MAX_EVENT_BYTES, NatsEventStoreConfig};
use crate::hex::encode_lower_hex;
use crate::stream_policy::{is_stream_not_found, stream_config_mismatches};

const LEGACY_EVENT_SCHEMA_VERSION: u16 = 1;
const CORRELATION_EVENT_SCHEMA_VERSION: u16 = 2;
const EVENT_SCHEMA_VERSION: u16 = 3;
const TRANSACTION_EVENT_SCHEMA_VERSION: u16 = 4;
const TRANSACTION_RECEIPT_SCHEMA_VERSION: u16 = 1;
const ATOMIC_BATCH_API_LEVEL: &str = "2";
const MINIMUM_ATOMIC_BATCH_SERVER_VERSION: (i64, i64, i64) = (2, 12, 1);
const NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT: &str =
    "Nats-Expected-Last-Subject-Sequence-Subject";

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
        validate_server_compatibility(&context)?;
        validate_server_payload_capacity(&context, &config)?;
        let stream = context
            .get_stream(config.stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        verify_stream_config(&config.stream_config(), &stream.cached_info().config)?;
        Ok(Self { context, config })
    }

    pub const fn config(&self) -> &NatsEventStoreConfig {
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
            let next_event_ordinal = decoded
                .event_ordinal
                .checked_add(1)
                .ok_or_else(|| corrupt("stored event has invalid commit coordinates"))?;
            if next_event_ordinal == decoded.event_count {
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
        if self
            .load_transaction_receipt_inner(stream_id, batch.operation_id())
            .await?
            .is_some()
        {
            return Err(identity_conflict(
                "operation identity was already used by an event transaction",
            ));
        }
        Self::resolve_existing(&history, batch)?.map_or_else(
            || Err(conflict("aggregate changed during append")),
            |events| Ok(AppendOutcome::ExactReplay(events)),
        )
    }

    async fn verify_published_commit(
        &self,
        stream_id: &StreamId,
        subject: &str,
        sequence: u64,
        commit_id: &CommitId,
        expected_events: &RecordedBatch,
    ) -> Result<(), EventStoreError> {
        let history = self.load_history(stream_id).await?;
        let stored = history
            .commits
            .iter()
            .find(|commit| commit.batch.commit_id() == commit_id)
            .ok_or_else(|| corrupt("published commit was not visible in aggregate history"))?;
        if stored.events.as_slice() != expected_events.events() {
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
        if &decoded.recorded != expected_events.last() {
            return Err(corrupt("PubAck sequence contains a different final event"));
        }
        Ok(())
    }

    async fn load_transaction_receipt_inner(
        &self,
        primary_stream_id: &StreamId,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        let subject = self
            .config
            .transaction_subject(primary_stream_id, operation_id.as_str());
        let stream = self
            .context
            .get_stream(self.config.stream_name())
            .await
            .map_err(|error| unavailable(format!("failed to get event-store stream: {error}")))?;
        let message = match stream.get_last_raw_message_by_subject(&subject).await {
            Ok(message) => message,
            Err(error) if error.kind() == LastRawMessageErrorKind::NoMessageFound => {
                return Ok(None);
            }
            Err(error) => {
                return Err(unavailable(format!(
                    "failed to locate transaction receipt: {error}"
                )));
            }
        };
        if message.subject.as_str() != subject {
            return Err(corrupt(
                "transaction receipt lookup returned the wrong subject",
            ));
        }
        let content = decode_transaction_receipt(&self.config, &message.headers, &message.payload)?;
        if content.operation_id != operation_id.as_str() {
            return Err(corrupt(
                "transaction receipt belongs to a different operation",
            ));
        }
        let receipt = self.materialize_transaction_receipt(content).await?;
        if receipt.primary_stream_id() != Some(primary_stream_id) {
            return Err(corrupt(
                "transaction receipt belongs to a different primary stream",
            ));
        }
        Ok(Some(receipt))
    }

    async fn materialize_transaction_receipt(
        &self,
        content: TransactionReceiptContentWire,
    ) -> Result<TransactionReceipt, EventStoreError> {
        let operation_id = OperationId::new(content.operation_id)
            .map_err(|error| corrupt(format!("invalid transaction operation identity: {error}")))?;
        let fingerprint =
            ContentFingerprint::from_hex(&content.operation_fingerprint).map_err(|error| {
                corrupt(format!(
                    "invalid transaction operation fingerprint: {error}"
                ))
            })?;
        let correlation_id = content
            .correlation_id
            .map(CorrelationId::new)
            .transpose()
            .map_err(|error| {
                corrupt(format!("invalid transaction correlation identity: {error}"))
            })?;
        let causation_id = content
            .causation_id
            .map(CausationId::new)
            .transpose()
            .map_err(|error| corrupt(format!("invalid transaction causation identity: {error}")))?;
        let mut seen = HashSet::with_capacity(content.participants.len());
        let mut streams = Vec::with_capacity(content.participants.len());
        for participant in content.participants {
            let stream_id = stream_id_from_wire(participant.stream)?;
            if !seen.insert(stream_id.clone()) {
                return Err(corrupt("transaction receipt repeats an aggregate stream"));
            }
            let base_version = StreamVersion::new(participant.base_stream_version);
            let history = self.load_history(&stream_id).await?;
            let first_version = base_version
                .value()
                .checked_add(1)
                .ok_or_else(|| corrupt("transaction receipt stream version overflowed"))?;
            let last_version = base_version
                .value()
                .checked_add(u64::from(participant.event_count))
                .ok_or_else(|| corrupt("transaction receipt stream version overflowed"))?;
            let events: Vec<_> = if participant.event_count == 0 {
                Vec::new()
            } else {
                history
                    .events
                    .into_iter()
                    .filter(|event| {
                        (first_version..=last_version).contains(&event.stream_version().value())
                    })
                    .collect()
            };
            let event_count = usize::try_from(participant.event_count)
                .map_err(|_| corrupt("transaction receipt event count cannot be represented"))?;
            if events.len() != event_count
                || events
                    .first()
                    .is_some_and(|event| event.stream_version().value() != first_version)
                || events
                    .last()
                    .is_some_and(|event| event.stream_version().value() != last_version)
                || events.iter().any(|event| {
                    event.operation_id() != &operation_id
                        || event.operation_fingerprint() != fingerprint
                        || event.correlation_id() != correlation_id.as_ref()
                        || event.causation_id() != causation_id.as_ref()
                })
                || participant.commit_id.as_deref()
                    != events.first().map(|event| event.commit_id().as_str())
            {
                return Err(corrupt(
                    "transaction receipt does not match its aggregate events",
                ));
            }
            streams.push(TransactionStreamReceipt::new(
                stream_id,
                base_version,
                events,
            ));
        }
        let mut receipt = TransactionReceipt::new(operation_id, fingerprint, streams);
        if let Some(correlation_id) = correlation_id {
            receipt = receipt.with_correlation_id(correlation_id);
        }
        if let Some(causation_id) = causation_id {
            receipt = receipt.with_causation_id(causation_id);
        }
        Ok(receipt)
    }

    async fn resolve_transaction_race(
        &self,
        transaction: &EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        let primary_stream_id = transaction
            .primary_stream_id()
            .ok_or_else(|| invalid("an event transaction must contain at least one participant"))?;
        match self
            .load_transaction_receipt_inner(primary_stream_id, transaction.operation_id())
            .await?
        {
            Some(receipt) if transaction_matches_receipt(transaction, &receipt) => {
                Ok(TransactionAppendOutcome::ExactReplay(receipt))
            }
            Some(_) => Err(identity_conflict(
                "transaction identity was reused with different content",
            )),
            None => Err(conflict("an aggregate changed during transaction append")),
        }
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
        if self
            .load_transaction_receipt_inner(stream_id, batch.operation_id())
            .await?
            .is_some()
        {
            return Err(identity_conflict(
                "operation identity was already used by an event transaction",
            ));
        }
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
        let payloads = encode_events(&self.config, stream_id, &batch, recorded.events())?;
        for payload in &payloads {
            if payload.len() > self.config.max_event_bytes() {
                return Err(invalid(format!(
                    "encoded event exceeds the configured {}-byte limit",
                    self.config.max_event_bytes()
                )));
            }
        }
        let recorded_count = u64::try_from(recorded.len())
            .map_err(|_| invalid("event batch count cannot be represented"))?;
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
            || ack.count != Some(recorded_count)
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
        Ok(AppendOutcome::Appended(recorded.into_events()))
    }

    async fn load_transaction_receipt(
        &self,
        primary_stream_id: &StreamId,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.load_transaction_receipt_inner(primary_stream_id, operation_id)
            .await
    }

    #[allow(clippy::too_many_lines)]
    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        validate_transaction_shape(&transaction)?;
        let domain_event_count = validate_transaction_item_limit(&transaction)?;
        let primary_stream_id = transaction
            .primary_stream_id()
            .ok_or_else(|| invalid("an event transaction must contain at least one participant"))?
            .clone();
        if let Some(receipt) = self
            .load_transaction_receipt_inner(&primary_stream_id, transaction.operation_id())
            .await?
        {
            if transaction_matches_receipt(&transaction, &receipt) {
                return Ok(TransactionAppendOutcome::ExactReplay(receipt));
            }
            return Err(identity_conflict(
                "transaction identity was reused with different content",
            ));
        }
        validate_transaction_participant_requests(&transaction)?;

        let mut staged = Vec::with_capacity(transaction.participants().len());
        for participant in transaction.participants() {
            let history = self.load_history(participant.stream_id()).await?;
            if participant.stream_id() == &primary_stream_id
                && history
                    .events
                    .iter()
                    .any(|event| event.operation_id() == transaction.operation_id())
            {
                return Err(identity_conflict(
                    "operation identity was already used without its transaction receipt",
                ));
            }
            if let Some(batch) = participant.batch()
                && Self::resolve_existing(&history, batch)?.is_some()
            {
                return Err(identity_conflict(
                    "operation identity was already used without its transaction receipt",
                ));
            }
            let base_version = history
                .events
                .last()
                .map_or(StreamVersion::ZERO, RecordedEvent::stream_version);
            validate_expected_version(participant.expected_version(), base_version)?;
            let recorded = participant
                .batch()
                .map(|batch| {
                    validate_derived_identities(participant.stream_id(), batch)?;
                    record_batch(participant.stream_id(), base_version, batch)
                })
                .transpose()?
                .map(RecordedBatch::into_events)
                .unwrap_or_default();
            staged.push(StagedTransactionParticipant {
                stream_id: participant.stream_id().clone(),
                base_version,
                last_subject_stream_sequence: history.last_subject_stream_sequence,
                batch: participant.batch().cloned(),
                recorded,
            });
        }

        let transaction_event_count = u32::try_from(domain_event_count)
            .map_err(|_| invalid("transaction event count cannot be represented"))?;

        let mut messages = Vec::with_capacity(domain_event_count);
        let mut transaction_offset = 0_u32;
        for participant in &staged {
            let Some(batch) = &participant.batch else {
                continue;
            };
            let subject = self.config.aggregate_subject(
                participant.stream_id.aggregate_type().as_str(),
                participant.stream_id.aggregate_id().as_str(),
            );
            let payloads = encode_transaction_events(
                &self.config,
                &participant.stream_id,
                batch,
                &participant.recorded,
                transaction_offset,
                transaction_event_count,
            )?;
            for (index, payload) in payloads.into_iter().enumerate() {
                if payload.len() > self.config.max_event_bytes() {
                    return Err(invalid(format!(
                        "encoded event exceeds the configured {}-byte limit",
                        self.config.max_event_bytes()
                    )));
                }
                messages.push(AtomicPublishMessage {
                    subject: subject.clone(),
                    payload,
                    expected_last_subject_sequence: (index == 0)
                        .then_some(participant.last_subject_stream_sequence),
                    expectation_subject: None,
                });
            }
            transaction_offset = transaction_offset
                .checked_add(
                    u32::try_from(participant.recorded.len())
                        .map_err(|_| invalid("participant event count cannot be represented"))?,
                )
                .ok_or_else(|| invalid("transaction event ordinal overflowed"))?;
        }

        for (ordinal, participant) in staged
            .iter()
            .filter(|participant| participant.batch.is_none())
            .enumerate()
        {
            let guarded_subject = self.config.aggregate_subject(
                participant.stream_id.aggregate_type().as_str(),
                participant.stream_id.aggregate_id().as_str(),
            );
            let payload = serde_json::to_vec(&TransactionGuardWire {
                operation_id: transaction.operation_id().as_str(),
                guarded_stream: stream_identity(&participant.stream_id),
            })
            .map_err(|error| invalid(format!("failed to encode transaction guard: {error}")))?;
            if payload.len() > self.config.max_event_bytes() {
                return Err(invalid(format!(
                    "encoded transaction guard exceeds the configured {}-byte limit",
                    self.config.max_event_bytes()
                )));
            }
            messages.push(AtomicPublishMessage {
                subject: self.config.transaction_guard_subject(
                    &primary_stream_id,
                    transaction.operation_id().as_str(),
                    ordinal,
                ),
                payload,
                expected_last_subject_sequence: Some(participant.last_subject_stream_sequence),
                expectation_subject: Some(guarded_subject),
            });
        }

        let receipt_content = transaction_receipt_content(&self.config, &transaction, &staged)?;
        let receipt_payload = encode_transaction_receipt(&receipt_content)?;
        if receipt_payload.len() > self.config.max_event_bytes() {
            return Err(invalid(format!(
                "encoded transaction receipt exceeds the configured {}-byte limit",
                self.config.max_event_bytes()
            )));
        }
        messages.push(AtomicPublishMessage {
            subject: self
                .config
                .transaction_subject(&primary_stream_id, transaction.operation_id().as_str()),
            payload: receipt_payload,
            expected_last_subject_sequence: Some(0),
            expectation_subject: None,
        });
        let transaction_item_count = messages.len();

        let batch_id = new_transaction_batch_id(&self.context.client(), transaction.operation_id());
        let ack =
            match publish_atomic_messages(&self.context, &self.config, &batch_id, messages).await {
                Ok(ack) => ack,
                Err(AtomicBatchPublishError::Expectation) => {
                    return self.resolve_transaction_race(&transaction).await;
                }
                Err(AtomicBatchPublishError::Store(error)) => {
                    if error.kind() == EventStoreErrorKind::Unavailable
                        && let Some(receipt) = self
                            .load_transaction_receipt_inner(
                                &primary_stream_id,
                                transaction.operation_id(),
                            )
                            .await?
                        && transaction_matches_receipt(&transaction, &receipt)
                    {
                        return Ok(TransactionAppendOutcome::ExactReplay(receipt));
                    }
                    return Err(error);
                }
            };
        if ack.stream != self.config.stream_name()
            || ack.sequence == 0
            || ack.batch.as_deref() != Some(batch_id.as_str())
            || ack.count
                != Some(
                    u64::try_from(transaction_item_count)
                        .map_err(|_| invalid("transaction item count cannot be represented"))?,
                )
        {
            return Err(corrupt(
                "atomic transaction PubAck returned incompatible stream, sequence, or batch metadata",
            ));
        }
        let receipt = self
            .load_transaction_receipt_inner(&primary_stream_id, transaction.operation_id())
            .await?
            .ok_or_else(|| corrupt("published transaction receipt is not visible"))?;
        if !transaction_matches_receipt(&transaction, &receipt) {
            return Err(corrupt(
                "published transaction receipt contains different content",
            ));
        }
        Ok(TransactionAppendOutcome::Appended(receipt))
    }
}

pub async fn provision_event_store(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
) -> Result<(), EventStoreError> {
    validate_server_compatibility(context)?;
    validate_server_payload_capacity(context, config)?;
    let mut expected = config.stream_config();
    match context.get_stream(config.stream_name()).await {
        Ok(existing) => {
            let actual = &existing.cached_info().config;
            let subjects = &actual.subjects;
            let legacy_subjects = vec![config.aggregate_subject_filter()];
            if subjects != &expected.subjects && subjects != &legacy_subjects {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::ConfigurationMismatch,
                    "existing event-store stream belongs to a different application or bounded context",
                ));
            }
            if actual.max_message_size == -1 || actual.max_message_size > expected.max_message_size
            {
                expected.max_message_size = actual.max_message_size;
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

struct RecordedBatch {
    events: Vec<RecordedEvent>,
    last: RecordedEvent,
}

impl RecordedBatch {
    fn new(events: Vec<RecordedEvent>) -> Result<Self, EventStoreError> {
        let last = events
            .last()
            .cloned()
            .ok_or_else(|| invalid("event batch is empty"))?;
        Ok(Self { events, last })
    }

    fn events(&self) -> &[RecordedEvent] {
        &self.events
    }

    const fn len(&self) -> usize {
        self.events.len()
    }

    const fn last(&self) -> &RecordedEvent {
        &self.last
    }

    fn into_events(self) -> Vec<RecordedEvent> {
        self.events
    }
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
    event_count: usize,
    events: Vec<RecordedEvent>,
}

pub struct DecodedEvent {
    pub batch_id: String,
    pub commit_id: CommitId,
    pub operation_id: OperationId,
    pub operation_fingerprint: ContentFingerprint,
    pub event_ordinal: usize,
    pub event_count: usize,
    #[allow(dead_code)]
    pub transaction_event_ordinal: usize,
    #[allow(dead_code)]
    pub transaction_event_count: usize,
    pub recorded: RecordedEvent,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transaction_event_ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transaction_event_count: Option<u32>,
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredTransactionReceiptWire {
    schema_version: u16,
    checksum: String,
    receipt: TransactionReceiptContentWire,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransactionReceiptContentWire {
    event_store_stream: String,
    application: String,
    bounded_context: String,
    operation_id: String,
    operation_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    causation_id: Option<String>,
    participants: Vec<TransactionParticipantWire>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransactionParticipantWire {
    stream: StreamIdentityWire,
    base_stream_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit_id: Option<String>,
    event_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptChecksumInput<'a> {
    schema_version: u16,
    receipt: &'a TransactionReceiptContentWire,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TransactionGuardWire<'a> {
    operation_id: &'a str,
    guarded_stream: StreamIdentityWire,
}

struct AtomicPublishMessage {
    subject: String,
    payload: Vec<u8>,
    expected_last_subject_sequence: Option<u64>,
    expectation_subject: Option<String>,
}

struct StagedTransactionParticipant {
    stream_id: StreamId,
    base_version: StreamVersion,
    last_subject_stream_sequence: u64,
    batch: Option<EventBatch>,
    recorded: Vec<RecordedEvent>,
}

fn encode_events(
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    batch: &EventBatch,
    recorded: &[RecordedEvent],
) -> Result<Vec<Vec<u8>>, EventStoreError> {
    encode_events_inner(config, stream_id, batch, recorded, None)
}

fn encode_transaction_events(
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    batch: &EventBatch,
    recorded: &[RecordedEvent],
    transaction_offset: u32,
    transaction_event_count: u32,
) -> Result<Vec<Vec<u8>>, EventStoreError> {
    encode_events_inner(
        config,
        stream_id,
        batch,
        recorded,
        Some((transaction_offset, transaction_event_count)),
    )
}

fn encode_events_inner(
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    batch: &EventBatch,
    recorded: &[RecordedEvent],
    transaction: Option<(u32, u32)>,
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
            let transaction_event_ordinal = transaction
                .map(|(offset, _)| {
                    offset
                        .checked_add(ordinal)
                        .ok_or_else(|| invalid("transaction event ordinal overflowed"))
                })
                .transpose()?;
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
                transaction_event_ordinal,
                transaction_event_count: transaction.map(|(_, count)| count),
                event_id: event.event_id().as_str().to_owned(),
                event_type: event.event_type().to_owned(),
                event_schema_version: event.schema_version(),
                payload_base64: STANDARD.encode(event.payload()),
            };
            let schema_version = if transaction.is_some() {
                TRANSACTION_EVENT_SCHEMA_VERSION
            } else {
                EVENT_SCHEMA_VERSION
            };
            let checksum = event_checksum(schema_version, &content)
                .map_err(|error| invalid(format!("failed to checksum event: {error}")))?;
            serde_json::to_vec(&StoredEventWire {
                schema_version,
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
pub fn decode_consumed_event(
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
    if payload.len()
        > config
            .max_event_bytes()
            .max(LEGACY_EVENT_STORE_MAX_EVENT_BYTES)
    {
        return Err(corrupt("stored event exceeds the supported byte limit"));
    }
    let wire: StoredEventWire = serde_json::from_slice(payload)
        .map_err(|error| corrupt(format!("stored event is not valid wire JSON: {error}")))?;
    if !matches!(
        wire.schema_version,
        LEGACY_EVENT_SCHEMA_VERSION
            | CORRELATION_EVENT_SCHEMA_VERSION
            | EVENT_SCHEMA_VERSION
            | TRANSACTION_EVENT_SCHEMA_VERSION
    ) {
        return Err(corrupt("stored event has an unsupported schema version"));
    }
    let maximum_event_bytes = if wire.schema_version < TRANSACTION_EVENT_SCHEMA_VERSION {
        LEGACY_EVENT_STORE_MAX_EVENT_BYTES
    } else {
        config.max_event_bytes()
    };
    if payload.len() > maximum_event_bytes {
        return Err(corrupt("stored event exceeds its schema byte limit"));
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
    if wire.schema_version < TRANSACTION_EVENT_SCHEMA_VERSION
        && (wire.event.transaction_event_ordinal.is_some()
            || wire.event.transaction_event_count.is_some())
    {
        return Err(corrupt(
            "legacy stored events cannot contain transaction coordinates",
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
    if wire.schema_version >= EVENT_SCHEMA_VERSION
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
    let event_count = usize::try_from(wire.event.commit_event_count)
        .map_err(|_| corrupt("stored event has invalid commit coordinates"))?;
    let event_ordinal = usize::try_from(wire.event.commit_event_ordinal)
        .map_err(|_| corrupt("stored event has invalid commit coordinates"))?;
    if event_count == 0 || event_count > MAX_EVENTS_PER_BATCH || event_ordinal >= event_count {
        return Err(corrupt("stored event has invalid commit coordinates"));
    }
    let (transaction_event_ordinal, transaction_event_count) =
        if wire.schema_version == TRANSACTION_EVENT_SCHEMA_VERSION {
            let ordinal = wire
                .event
                .transaction_event_ordinal
                .ok_or_else(|| corrupt("transactional event has no transaction ordinal"))?;
            let count = wire
                .event
                .transaction_event_count
                .ok_or_else(|| corrupt("transactional event has no transaction event count"))?;
            let count = usize::try_from(count)
                .map_err(|_| corrupt("transactional event has invalid transaction coordinates"))?;
            let ordinal = usize::try_from(ordinal)
                .map_err(|_| corrupt("transactional event has invalid transaction coordinates"))?;
            if count == 0 || count > MAX_EVENTS_PER_BATCH || ordinal >= count {
                return Err(corrupt(
                    "transactional event has invalid transaction coordinates",
                ));
            }
            (ordinal, count)
        } else {
            (event_ordinal, event_count)
        };
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
        wire.schema_version,
        event_ordinal,
        event_count,
        transaction_event_ordinal,
        expected_last_subject_sequence,
    )?;

    Ok(DecodedEvent {
        batch_id,
        commit_id,
        operation_id,
        operation_fingerprint,
        event_ordinal,
        event_count,
        transaction_event_ordinal,
        transaction_event_count,
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
        if let Some(pending) = self.pending.take_if(|pending| pending.is_complete()) {
            let stored = pending.finish()?;
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
        let expected_ordinal = self.events.len();
        let first = self
            .events
            .first()
            .ok_or_else(|| corrupt("stored commit is empty"))?;
        if decoded.event_ordinal != expected_ordinal
            || decoded.event_count != self.event_count
            || decoded.batch_id != self.batch_id
            || decoded.commit_id != self.commit_id
            || decoded.operation_id != self.operation_id
            || decoded.operation_fingerprint != self.operation_fingerprint
            || decoded.recorded.correlation_id() != first.correlation_id()
            || decoded.recorded.causation_id() != first.causation_id()
        {
            return Err(corrupt("stored commit metadata is inconsistent"));
        }
        self.events.push(decoded.recorded);
        Ok(())
    }

    const fn is_complete(&self) -> bool {
        self.events.len() == self.event_count
    }

    fn finish(self) -> Result<StoredCommit, EventStoreError> {
        let first = self
            .events
            .first()
            .ok_or_else(|| corrupt("stored commit is empty"))?;
        let correlation_id = first.correlation_id().cloned();
        let causation_id = first.causation_id().cloned();
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
        if let Some(correlation_id) = correlation_id {
            batch = batch.with_correlation_id(correlation_id);
        }
        if let Some(causation_id) = causation_id {
            batch = batch.with_causation_id(causation_id);
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
    let messages = payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| AtomicPublishMessage {
            subject: subject.to_owned(),
            payload,
            expected_last_subject_sequence: (index == 0).then_some(expected_last_subject_sequence),
            expectation_subject: None,
        })
        .collect();
    publish_atomic_messages(context, config, batch_id, messages).await
}

async fn publish_atomic_messages(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
    batch_id: &str,
    messages: Vec<AtomicPublishMessage>,
) -> Result<AtomicPublishAck, AtomicBatchPublishError> {
    let message_count = messages.len();
    for (index, atomic) in messages.into_iter().enumerate() {
        let one_based_sequence = index.checked_add(1).ok_or_else(|| {
            AtomicBatchPublishError::Store(EventStoreError::new(
                EventStoreErrorKind::CapacityExhausted,
                "atomic batch sequence space is exhausted",
            ))
        })?;
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert(NATS_REQUIRED_API_LEVEL, ATOMIC_BATCH_API_LEVEL);
        headers.insert(NATS_BATCH_ID, batch_id);
        headers.insert(NATS_BATCH_SEQUENCE, one_based_sequence.to_string());
        if index == 0 {
            headers.insert(NATS_EXPECTED_STREAM, config.stream_name());
        }
        if let Some(expected) = atomic.expected_last_subject_sequence {
            headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, expected.to_string());
        }
        if let Some(subject) = atomic.expectation_subject {
            headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT, subject);
        }
        if one_based_sequence == message_count {
            headers.insert(NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL);
        }

        let message = context
            .client()
            .send_request(
                atomic.subject,
                Request::new()
                    .headers(headers)
                    .payload(atomic.payload.into())
                    .timeout(Some(config.puback_timeout())),
            )
            .await
            .map_err(|error| {
                AtomicBatchPublishError::Store(unavailable(format!(
                    "atomic event publish failed: {error}"
                )))
            })?;

        if one_based_sequence != message_count {
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
        || code == jetstream::ErrorCode::STREAM_WRONG_LAST_SEQUENCE_CONSTANT
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
    } else if code == jetstream::ErrorCode::STREAM_MESSAGE_EXCEEDS_MAXIMUM {
        AtomicBatchPublishError::Store(invalid(
            "encoded event plus NATS headers exceeds the configured message-size limit",
        ))
    } else {
        AtomicBatchPublishError::Store(unavailable(format!(
            "atomic event publish was rejected: {error}"
        )))
    }
}

fn validate_server_payload_capacity(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
) -> Result<(), EventStoreError> {
    let negotiated = context.client().max_payload();
    let required = config.max_wire_message_bytes();
    if negotiated < required {
        return Err(EventStoreError::new(
            EventStoreErrorKind::ConfigurationMismatch,
            format!(
                "NATS max_payload is {negotiated} bytes, but the configured event-store message limit requires {required} bytes",
            ),
        ));
    }
    Ok(())
}

fn validate_server_compatibility(context: &jetstream::Context) -> Result<(), EventStoreError> {
    let (major, minor, patch) = MINIMUM_ATOMIC_BATCH_SERVER_VERSION;
    if !context.client().is_server_compatible(major, minor, patch) {
        return Err(EventStoreError::new(
            EventStoreErrorKind::ConfigurationMismatch,
            "NATS Server 2.12.1 or newer is required for cross-subject atomic event batches",
        ));
    }
    Ok(())
}

fn new_atomic_batch_id(client: &async_nats::Client, commit_id: &CommitId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(client.new_inbox().as_bytes());
    hasher.update(commit_id.as_str().as_bytes());
    encode_lower_hex(hasher.finalize())
}

fn new_transaction_batch_id(client: &async_nats::Client, operation_id: &OperationId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(client.new_inbox().as_bytes());
    hasher.update(operation_id.as_str().as_bytes());
    encode_lower_hex(hasher.finalize())
}

fn validate_atomic_headers(
    stream_name: &str,
    headers: &HeaderMap,
    schema_version: u16,
    commit_event_ordinal: usize,
    commit_event_count: usize,
    transaction_event_ordinal: usize,
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
        .parse::<usize>()
        .map_err(|_| corrupt("stored event has an invalid atomic batch sequence"))?;
    let one_based_sequence = transaction_event_ordinal
        .checked_add(1)
        .ok_or_else(|| corrupt("stored event has an invalid atomic batch sequence"))?;
    if batch_sequence != one_based_sequence {
        return Err(corrupt(
            "stored event atomic batch sequence does not match its transaction ordinal",
        ));
    }
    let expected_stream = optional_single_header(headers, "Nats-Expected-Stream")?;
    let expected_sequence = optional_single_header(headers, "Nats-Expected-Last-Subject-Sequence")?;
    if commit_event_ordinal == 0 {
        if transaction_event_ordinal == 0 && expected_stream != Some(stream_name) {
            return Err(corrupt("stored commit has an incompatible expected stream"));
        }
        if transaction_event_ordinal != 0 && expected_stream.is_some() {
            return Err(corrupt("stored transaction repeats its expected stream"));
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
    if schema_version == TRANSACTION_EVENT_SCHEMA_VERSION {
        if commit.is_some() {
            return Err(corrupt(
                "transactional domain event finalized before its receipt",
            ));
        }
    } else if commit_event_ordinal
        .checked_add(1)
        .is_some_and(|ordinal| ordinal == commit_event_count)
    {
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
) -> Result<RecordedBatch, EventStoreError> {
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
    RecordedBatch::new(recorded)
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

fn validate_transaction_shape(transaction: &EventTransaction) -> Result<(), EventStoreError> {
    if transaction.participants().is_empty() {
        return Err(invalid(
            "an event transaction must contain at least one participant",
        ));
    }
    if transaction
        .participants()
        .first()
        .is_some_and(|participant| participant.batch().is_none())
    {
        return Err(invalid(
            "an event transaction's primary participant must contain an event batch",
        ));
    }
    let mut streams = HashSet::with_capacity(transaction.participants().len());
    for participant in transaction.participants() {
        if !streams.insert(participant.stream_id()) {
            return Err(invalid(
                "an event transaction must not contain duplicate streams",
            ));
        }
    }
    Ok(())
}

fn validate_transaction_participant_requests(
    transaction: &EventTransaction,
) -> Result<(), EventStoreError> {
    for participant in transaction.participants() {
        if matches!(
            participant.expected_version(),
            ExpectedVersion::Exact(StreamVersion::ZERO)
        ) {
            return Err(invalid(
                "Exact requires a non-zero stream version; use NoStream for an absent stream",
            ));
        }
        let Some(batch) = participant.batch() else {
            continue;
        };
        if batch.operation_id() != transaction.operation_id()
            || batch.operation_fingerprint() != transaction.operation_fingerprint()
            || batch.correlation_id() != transaction.correlation_id()
            || batch.causation_id() != transaction.causation_id()
        {
            return Err(invalid(
                "participant commit metadata does not match its transaction",
            ));
        }
    }
    Ok(())
}

fn validate_expected_version(
    expected: ExpectedVersion,
    current: StreamVersion,
) -> Result<(), EventStoreError> {
    match expected {
        ExpectedVersion::NoStream if current == StreamVersion::ZERO => Ok(()),
        ExpectedVersion::Exact(version) if version == StreamVersion::ZERO => Err(invalid(
            "Exact requires a non-zero stream version; use NoStream for an absent stream",
        )),
        ExpectedVersion::Exact(version) if version == current => Ok(()),
        ExpectedVersion::NoStream | ExpectedVersion::Exact(_) => Err(conflict(format!(
            "expected version does not match current version {}",
            current.value()
        ))),
    }
}

fn transaction_receipt_content(
    config: &NatsEventStoreConfig,
    transaction: &EventTransaction,
    staged: &[StagedTransactionParticipant],
) -> Result<TransactionReceiptContentWire, EventStoreError> {
    Ok(TransactionReceiptContentWire {
        event_store_stream: config.stream_name().to_owned(),
        application: config.application().as_str().to_owned(),
        bounded_context: config.bounded_context().as_str().to_owned(),
        operation_id: transaction.operation_id().as_str().to_owned(),
        operation_fingerprint: transaction.operation_fingerprint().to_hex(),
        correlation_id: transaction
            .correlation_id()
            .map(|identity| identity.as_str().to_owned()),
        causation_id: transaction
            .causation_id()
            .map(|identity| identity.as_str().to_owned()),
        participants: staged
            .iter()
            .map(|participant| {
                Ok(TransactionParticipantWire {
                    stream: stream_identity(&participant.stream_id),
                    base_stream_version: participant.base_version.value(),
                    commit_id: participant
                        .batch
                        .as_ref()
                        .map(|batch| batch.commit_id().as_str().to_owned()),
                    event_count: u32::try_from(participant.recorded.len()).map_err(|_| {
                        invalid("transaction participant event count cannot be represented")
                    })?,
                })
            })
            .collect::<Result<Vec<_>, EventStoreError>>()?,
    })
}

fn encode_transaction_receipt(
    content: &TransactionReceiptContentWire,
) -> Result<Vec<u8>, EventStoreError> {
    let checksum = transaction_receipt_checksum(TRANSACTION_RECEIPT_SCHEMA_VERSION, content)
        .map_err(|error| invalid(format!("failed to checksum transaction receipt: {error}")))?;
    serde_json::to_vec(&StoredTransactionReceiptWire {
        schema_version: TRANSACTION_RECEIPT_SCHEMA_VERSION,
        checksum,
        receipt: content.clone(),
    })
    .map_err(|error| invalid(format!("failed to encode transaction receipt: {error}")))
}

fn decode_transaction_receipt(
    config: &NatsEventStoreConfig,
    headers: &HeaderMap,
    payload: &[u8],
) -> Result<TransactionReceiptContentWire, EventStoreError> {
    if payload.len() > config.max_event_bytes() {
        return Err(corrupt(
            "stored transaction receipt exceeds the configured byte limit",
        ));
    }
    if required_single_header(headers, "Content-Type")? != "application/json"
        || optional_single_header(headers, "Nats-Batch-Commit")? != Some(NATS_BATCH_COMMIT_FINAL)
        || optional_single_header(headers, "Nats-Expected-Last-Subject-Sequence")? != Some("0")
    {
        return Err(corrupt(
            "stored transaction receipt has incompatible atomic headers",
        ));
    }
    required_single_header(headers, "Nats-Batch-Id")?;
    let batch_sequence = required_single_header(headers, "Nats-Batch-Sequence")?
        .parse::<usize>()
        .map_err(|_| corrupt("stored transaction receipt has an invalid batch sequence"))?;
    if batch_sequence == 0 {
        return Err(corrupt(
            "stored transaction receipt has an invalid batch sequence",
        ));
    }
    let wire: StoredTransactionReceiptWire = serde_json::from_slice(payload).map_err(|error| {
        corrupt(format!(
            "stored transaction receipt is invalid JSON: {error}"
        ))
    })?;
    if wire.schema_version != TRANSACTION_RECEIPT_SCHEMA_VERSION {
        return Err(corrupt(
            "stored transaction receipt has an unsupported schema version",
        ));
    }
    let checksum =
        transaction_receipt_checksum(wire.schema_version, &wire.receipt).map_err(|error| {
            corrupt(format!(
                "stored transaction receipt cannot be checksummed: {error}"
            ))
        })?;
    if checksum != wire.checksum {
        return Err(corrupt(
            "stored transaction receipt checksum does not match its content",
        ));
    }
    if wire.receipt.event_store_stream != config.stream_name()
        || wire.receipt.application != config.application().as_str()
        || wire.receipt.bounded_context != config.bounded_context().as_str()
        || wire.receipt.participants.is_empty()
    {
        return Err(corrupt(
            "stored transaction receipt belongs to another event store or has no participants",
        ));
    }
    let expected_final_sequence =
        wire.receipt
            .participants
            .iter()
            .try_fold(1_usize, |item_count, participant| {
                let event_count = usize::try_from(participant.event_count).map_err(|_| {
                    corrupt("stored transaction receipt batch sequence calculation overflowed")
                })?;
                item_count.checked_add(event_count.max(1)).ok_or_else(|| {
                    corrupt("stored transaction receipt batch sequence calculation overflowed")
                })
            })?;
    if batch_sequence != expected_final_sequence {
        return Err(corrupt(
            "stored transaction receipt batch sequence does not match its participants",
        ));
    }
    Ok(wire.receipt)
}

fn transaction_receipt_checksum(
    schema_version: u16,
    receipt: &TransactionReceiptContentWire,
) -> Result<String, serde_json::Error> {
    let input = serde_json::to_vec(&ReceiptChecksumInput {
        schema_version,
        receipt,
    })?;
    Ok(encode_lower_hex(Sha256::digest(input)))
}

fn stream_identity(stream_id: &StreamId) -> StreamIdentityWire {
    StreamIdentityWire {
        aggregate_type: stream_id.aggregate_type().as_str().to_owned(),
        aggregate_id: stream_id.aggregate_id().as_str().to_owned(),
    }
}

fn stream_id_from_wire(wire: StreamIdentityWire) -> Result<StreamId, EventStoreError> {
    let aggregate_type = AggregateType::new(wire.aggregate_type)
        .map_err(|error| corrupt(format!("invalid receipt aggregate type: {error}")))?;
    let aggregate_id = AggregateId::new(wire.aggregate_id)
        .map_err(|error| corrupt(format!("invalid receipt aggregate identity: {error}")))?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
}

fn transaction_matches_receipt(
    transaction: &EventTransaction,
    receipt: &TransactionReceipt,
) -> bool {
    let metadata_matches = transaction.operation_id() == receipt.operation_id()
        && transaction.operation_fingerprint() == receipt.operation_fingerprint()
        && transaction.correlation_id() == receipt.correlation_id()
        && transaction.causation_id() == receipt.causation_id();
    metadata_matches
        && transaction.participants().len() == receipt.streams().len()
        && transaction
            .participants()
            .iter()
            .zip(receipt.streams())
            .all(|(participant, stored)| {
                participant.stream_id() == stored.stream_id()
                    && participant.batch().map_or_else(
                        || stored.events().is_empty(),
                        |batch| batch_matches_recorded(batch, stored.events()),
                    )
            })
}

fn batch_matches_recorded(batch: &EventBatch, recorded: &[RecordedEvent]) -> bool {
    batch.events().len() == recorded.len()
        && batch
            .events()
            .iter()
            .zip(recorded)
            .all(|(incoming, stored)| {
                incoming.event_id() == stored.event_id()
                    && incoming.event_type() == stored.event_type()
                    && incoming.schema_version() == stored.schema_version()
                    && incoming.payload() == stored.payload()
                    && batch.commit_id() == stored.commit_id()
                    && batch.operation_id() == stored.operation_id()
                    && batch.operation_fingerprint() == stored.operation_fingerprint()
                    && batch.correlation_id() == stored.correlation_id()
                    && batch.causation_id() == stored.causation_id()
            })
}

fn event_checksum(
    schema_version: u16,
    event: &StoredEventContentWire,
) -> Result<String, serde_json::Error> {
    let input = serde_json::to_vec(&ChecksumInput {
        schema_version,
        event,
    })?;
    Ok(encode_lower_hex(Sha256::digest(input)))
}

fn verify_stream_config(expected: &Config, actual: &Config) -> Result<(), EventStoreError> {
    let mut compatible = expected.clone();
    if actual.max_message_size == -1 || actual.max_message_size > expected.max_message_size {
        compatible.max_message_size = actual.max_message_size;
    }
    let mismatches = stream_config_mismatches(&compatible, actual);
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

    fn transaction_receipt_fixture() -> TransactionReceiptContentWire {
        TransactionReceiptContentWire {
            event_store_stream: "EVENTS".to_owned(),
            application: "acme".to_owned(),
            bounded_context: "orders".to_owned(),
            operation_id: "receipt-operation".to_owned(),
            operation_fingerprint: ContentFingerprint::digest("receipt-content").to_hex(),
            correlation_id: None,
            causation_id: None,
            participants: vec![
                TransactionParticipantWire {
                    stream: StreamIdentityWire {
                        aggregate_type: "Test".to_owned(),
                        aggregate_id: "one".to_owned(),
                    },
                    base_stream_version: 0,
                    commit_id: Some("receipt-commit".to_owned()),
                    event_count: 2,
                },
                TransactionParticipantWire {
                    stream: StreamIdentityWire {
                        aggregate_type: "Test".to_owned(),
                        aggregate_id: "observed".to_owned(),
                    },
                    base_stream_version: 1,
                    commit_id: None,
                    event_count: 0,
                },
            ],
        }
    }

    fn transaction_receipt_headers(batch_sequence: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert(NATS_BATCH_ID, "receipt-batch");
        headers.insert(NATS_BATCH_SEQUENCE, batch_sequence);
        headers.insert(NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL);
        headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, "0");
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
        let payload = encode_events(&config, &stream_id, &batch, recorded.events())
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

    #[test]
    fn schema_four_transaction_coordinates_round_trip() {
        let config = config();
        let stream_id = stream_id();
        let operation_id = OperationId::new("schema-4-operation").unwrap();
        let fingerprint = ContentFingerprint::digest("schema-4-content");
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
        let payload =
            encode_transaction_events(&config, &stream_id, &batch, recorded.events(), 0, 1)
                .unwrap()
                .remove(0);
        let wire: StoredEventWire = serde_json::from_slice(&payload).unwrap();
        assert_eq!(wire.schema_version, TRANSACTION_EVENT_SCHEMA_VERSION);
        assert_eq!(wire.event.transaction_event_ordinal, Some(0));
        assert_eq!(wire.event.transaction_event_count, Some(1));

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert(NATS_BATCH_ID, "schema-4-transaction");
        headers.insert(NATS_BATCH_SEQUENCE, "1");
        headers.insert(NATS_EXPECTED_STREAM, config.stream_name());
        headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, "0");
        let subject = config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let decoded = decode_event(&config, &subject, &stream_id, Some(0), &headers, &payload)
            .expect("schema-4 decode");
        assert_eq!(decoded.transaction_event_ordinal, 0);
        assert_eq!(decoded.transaction_event_count, 1);
        assert_eq!(&decoded.recorded, recorded.last());
    }

    #[test]
    fn transaction_receipt_batch_sequence_matches_all_transaction_items() {
        let config = config();
        let payload = encode_transaction_receipt(&transaction_receipt_fixture()).unwrap();

        decode_transaction_receipt(&config, &transaction_receipt_headers("4"), &payload)
            .expect("two events, one read guard, and one receipt use four batch items");

        for sequence in ["0", "3", "5", "184467440737095516160"] {
            let error = decode_transaction_receipt(
                &config,
                &transaction_receipt_headers(sequence),
                &payload,
            )
            .err()
            .expect("an invalid final receipt sequence must be rejected");
            assert_eq!(error.kind(), EventStoreErrorKind::CorruptHistory);
        }
    }

    #[test]
    fn transaction_shape_rejects_a_read_only_primary() {
        let primary = stream_id();
        let secondary = StreamId::new(
            AggregateType::new("Test").unwrap(),
            AggregateId::new("secondary").unwrap(),
        );
        let operation_id = OperationId::new("read-only-primary").unwrap();
        let fingerprint = ContentFingerprint::digest("read-only-primary");
        let metadata = ExecutionMetadata::new(secondary.clone(), operation_id.clone(), fingerprint);
        let batch = EventBatch::new(
            metadata.commit_id().clone(),
            operation_id.clone(),
            fingerprint,
            vec![NewEvent::new(metadata.event_id(0), "opened", 1, Vec::new()).unwrap()],
        )
        .unwrap();
        let transaction = EventTransaction::new(
            operation_id,
            fingerprint,
            vec![
                rostfrei_core::TransactionParticipant::new(
                    primary,
                    ExpectedVersion::NoStream,
                    None,
                ),
                rostfrei_core::TransactionParticipant::new(
                    secondary,
                    ExpectedVersion::NoStream,
                    Some(batch),
                ),
            ],
        );

        assert_eq!(
            validate_transaction_shape(&transaction)
                .expect_err("a read-only primary must be rejected")
                .kind(),
            EventStoreErrorKind::InvalidRequest
        );
    }

    #[test]
    fn legacy_schemas_retain_the_previous_read_limit_but_schema_four_does_not() {
        let config = config();
        assert_eq!(config.max_event_bytes(), 512 * 1024);
        let stream_id = stream_id();
        let operation_id = OperationId::new("large-schema-operation").unwrap();
        let fingerprint = ContentFingerprint::digest("large-schema-content");
        let metadata = ExecutionMetadata::new(stream_id.clone(), operation_id.clone(), fingerprint);
        let event = NewEvent::new(metadata.event_id(0), "opened", 1, vec![42; 400 * 1024]).unwrap();
        let batch = EventBatch::new(
            metadata.commit_id().clone(),
            operation_id,
            fingerprint,
            vec![event],
        )
        .unwrap();
        let recorded = record_batch(&stream_id, StreamVersion::ZERO, &batch).unwrap();
        let subject = config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );

        let legacy_payload = encode_events(&config, &stream_id, &batch, recorded.events())
            .unwrap()
            .remove(0);
        assert!(legacy_payload.len() > config.max_event_bytes());
        assert!(legacy_payload.len() <= LEGACY_EVENT_STORE_MAX_EVENT_BYTES);
        decode_event(
            &config,
            &subject,
            &stream_id,
            Some(0),
            &atomic_headers(config.stream_name()),
            &legacy_payload,
        )
        .expect("schema 1-3 history should retain the previous read limit");

        let transaction_payload =
            encode_transaction_events(&config, &stream_id, &batch, recorded.events(), 0, 1)
                .unwrap()
                .remove(0);
        assert!(transaction_payload.len() > config.max_event_bytes());
        let mut transaction_headers = HeaderMap::new();
        transaction_headers.insert("Content-Type", "application/json");
        transaction_headers.insert(NATS_BATCH_ID, "large-schema-transaction");
        transaction_headers.insert(NATS_BATCH_SEQUENCE, "1");
        transaction_headers.insert(NATS_EXPECTED_STREAM, config.stream_name());
        transaction_headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, "0");
        let Err(error) = decode_event(
            &config,
            &subject,
            &stream_id,
            Some(0),
            &transaction_headers,
            &transaction_payload,
        ) else {
            panic!("schema 4 history must obey the configured event limit");
        };
        assert_eq!(error.kind(), EventStoreErrorKind::CorruptHistory);
        assert!(error.message().contains("schema byte limit"));
    }

    #[test]
    fn transaction_replay_ignores_expectations_but_preserves_participant_shape() {
        let primary = stream_id();
        let observed = StreamId::new(
            AggregateType::new("Test").unwrap(),
            AggregateId::new("observed").unwrap(),
        );
        let operation_id = OperationId::new("retry-operation").unwrap();
        let fingerprint = ContentFingerprint::digest("retry-content");
        let metadata = ExecutionMetadata::new(primary.clone(), operation_id.clone(), fingerprint);
        let batch = EventBatch::new(
            metadata.commit_id().clone(),
            operation_id.clone(),
            fingerprint,
            vec![NewEvent::new(metadata.event_id(0), "opened", 1, b"{}".to_vec()).unwrap()],
        )
        .unwrap();
        let recorded = record_batch(&primary, StreamVersion::ZERO, &batch).unwrap();
        let receipt = TransactionReceipt::new(
            operation_id.clone(),
            fingerprint,
            vec![
                TransactionStreamReceipt::new(
                    primary.clone(),
                    StreamVersion::ZERO,
                    recorded.into_events(),
                ),
                TransactionStreamReceipt::new(observed.clone(), StreamVersion::new(7), Vec::new()),
            ],
        );
        let retry = EventTransaction::new(
            operation_id.clone(),
            fingerprint,
            vec![
                rostfrei_core::TransactionParticipant::new(
                    primary.clone(),
                    ExpectedVersion::Exact(StreamVersion::new(99)),
                    Some(batch),
                ),
                rostfrei_core::TransactionParticipant::new(
                    observed.clone(),
                    ExpectedVersion::NoStream,
                    None,
                ),
            ],
        );
        assert!(transaction_matches_receipt(&retry, &receipt));

        let changed_shape = EventTransaction::new(
            operation_id,
            fingerprint,
            vec![
                rostfrei_core::TransactionParticipant::new(
                    primary,
                    ExpectedVersion::NoStream,
                    None,
                ),
                rostfrei_core::TransactionParticipant::new(
                    observed,
                    ExpectedVersion::Exact(StreamVersion::new(7)),
                    None,
                ),
            ],
        );
        assert!(!transaction_matches_receipt(&changed_shape, &receipt));
    }

    #[test]
    fn larger_existing_stream_message_capacity_is_compatible() {
        let expected = config().stream_config();
        let mut actual = expected.clone();
        actual.max_message_size = expected
            .max_message_size
            .checked_add(1)
            .expect("test stream capacity should be incrementable");
        verify_stream_config(&expected, &actual).expect("larger legacy capacity is compatible");

        actual.max_message_size = expected
            .max_message_size
            .checked_sub(1)
            .expect("test stream capacity should be decrementable");
        assert_eq!(
            verify_stream_config(&expected, &actual)
                .expect_err("smaller stream capacity must be rejected")
                .kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
    }
}
