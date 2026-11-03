//! Bounded journal-local receipt resolution. This never loads aggregate histories
//! or consults retainable outcome state.
use super::fixture_manifest::decode_fixture_manifest;
use super::*;
use rostfrei_core::{DomainEventRef, OutcomeKey, OutcomeMarker};
use rostfrei_messaging_core::MessageTimestamp;

pub fn validate_authority(
    config: &NatsEventStoreConfig,
    journal: &jetstream::stream::Info,
) -> Result<(), EventStoreError> {
    outcomes::historical_authority(config, journal).map(|_| ())
}

/// The caller charges the receipt before entering. Every additional raw read,
/// including guards and guard checkpoints, is charged before decoding. A receipt
/// is validated completely before any of its references enter the shared result.
pub async fn inspect<F>(
    config: &NatsEventStoreConfig,
    journal: &jetstream::stream::Stream,
    message: &jetstream::Message,
    admit: F,
) -> Result<Option<(String, Vec<DomainEventRef>)>, EventStoreError>
where
    F: Fn(usize) -> Result<bool, EventStoreError> + Send + Sync,
{
    let info = message
        .info()
        .map_err(|_| corrupt("invalid receipt delivery metadata"))?;
    if info.stream != config.stream_name()
        || info.stream_sequence == 0
        || info.stream_sequence > journal.cached_info().state.last_sequence
    {
        return Err(corrupt("receipt is outside its captured journal"));
    }
    let headers = message
        .headers
        .as_ref()
        .ok_or_else(|| corrupt("receipt has no headers"))?;
    let fixture_prefix = format!("{}.fixture.", config.subject_prefix());
    if message.subject.as_str().starts_with(&fixture_prefix) {
        let manifest = decode_fixture_manifest(
            config,
            message.subject.as_str(),
            info.stream_sequence,
            headers,
            &message.payload,
        )?;
        let id = manifest
            .correlation_id
            .as_ref()
            .ok_or_else(|| corrupt("selected fixture has no correlation"))?
            .as_str()
            .to_owned();
        let mut references = Vec::new();
        for sequence in manifest.event_sequences.clone() {
            let Some(raw) = read_raw(journal, sequence, &admit).await? else {
                return Ok(None);
            };
            let decoded = decode_raw_event(config, &raw)?;
            manifest.validate_event(&decoded, sequence)?;
            references.push(reference(config, &decoded.recorded, raw.time)?);
        }
        return Ok(Some((id, references)));
    }
    let decoded =
        decode_transaction_receipt(config, info.stream_sequence, headers, &message.payload)?;
    if message.subject.as_str()
        != config.transaction_receipt_subject(
            &decoded.content.operation_id,
            decoded.content.correlation_id.as_deref(),
            decoded.content.causation_id.as_deref(),
        )
        || optional_single_header(headers, CORRELATION_ID_HEADER)?
            != decoded.content.correlation_id.as_deref()
    {
        return Err(corrupt(
            "transaction receipt subject or correlation header disagrees with its content",
        ));
    }
    inspect_transaction(config, journal, decoded, &admit).await
}

#[allow(clippy::too_many_lines)]
async fn inspect_transaction<F>(
    config: &NatsEventStoreConfig,
    journal: &jetstream::stream::Stream,
    decoded: DecodedTransactionReceipt,
    admit: &F,
) -> Result<Option<(String, Vec<DomainEventRef>)>, EventStoreError>
where
    F: Fn(usize) -> Result<bool, EventStoreError> + Sync,
{
    let preceding = decoded
        .batch_sequence
        .checked_sub(1)
        .and_then(|count| u64::try_from(count).ok())
        .ok_or_else(|| corrupt("receipt batch sequence overflow"))?;
    let start = decoded
        .stream_sequence
        .checked_sub(preceding)
        .filter(|sequence| *sequence > 0)
        .ok_or_else(|| corrupt("receipt batch start underflow"))?;
    let content = decoded.content;
    let operation =
        OperationId::new(content.operation_id).map_err(|error| corrupt(error.to_string()))?;
    let fingerprint = ContentFingerprint::from_hex(&content.operation_fingerprint)
        .map_err(|error| corrupt(error.to_string()))?;
    let correlation = content
        .correlation_id
        .map(CorrelationId::new)
        .transpose()
        .map_err(|error| corrupt(error.to_string()))?
        .ok_or_else(|| corrupt("selected transaction receipt has no correlation"))?;
    let cause = content
        .causation_id
        .map(CausationId::new)
        .transpose()
        .map_err(|error| corrupt(error.to_string()))?;
    let event_count = content
        .participants
        .iter()
        .try_fold(0_usize, |total, participant| {
            total
                .checked_add(
                    usize::try_from(participant.event_count)
                        .map_err(|_| corrupt("receipt event count overflow"))?,
                )
                .ok_or_else(|| corrupt("receipt event count overflow"))
        })?;
    if event_count > MAX_EVENTS_PER_BATCH {
        return Err(corrupt("receipt exceeds supported transaction size"));
    }
    let mut streams = Vec::with_capacity(content.participants.len());
    let mut seen = HashSet::new();
    let mut references = Vec::new();
    let mut ordinal = 0_usize;
    let mut guard_ordinal = 0_usize;
    for participant in content.participants {
        let stream_id = stream_id_from_wire(participant.stream)?;
        if !seen.insert(stream_id.clone()) {
            return Err(corrupt("receipt repeats an aggregate stream"));
        }
        let base_version = StreamVersion::new(participant.base_stream_version);
        let count = usize::try_from(participant.event_count)
            .map_err(|_| corrupt("participant event count overflow"))?;
        let mut events = Vec::new();
        if count == 0 {
            let guard = Guard {
                stream_id: &stream_id,
                base_version,
                operation: &operation,
                batch_id: &decoded.batch_id,
                start,
                ordinal: event_count
                    .checked_add(guard_ordinal)
                    .ok_or_else(|| corrupt("guard ordinal overflow"))?,
                index: guard_ordinal,
            };
            if !inspect_guard(config, journal, guard, admit).await? {
                return Ok(None);
            }
            guard_ordinal = guard_ordinal
                .checked_add(1)
                .ok_or_else(|| corrupt("guard ordinal overflow"))?;
        } else {
            let commit = derive_commit_id(&stream_id, &operation);
            if participant.commit_id.as_deref() != Some(commit.as_str()) {
                return Err(corrupt("receipt has incompatible derived commit identity"));
            }
            for event_ordinal in 0..count {
                let sequence = start
                    .checked_add(
                        u64::try_from(ordinal).map_err(|_| corrupt("event position overflow"))?,
                    )
                    .ok_or_else(|| corrupt("event position overflow"))?;
                let Some(raw) = read_raw(journal, sequence, admit).await? else {
                    return Ok(None);
                };
                let event = decode_raw_event(config, &raw)?;
                let version = base_version
                    .value()
                    .checked_add(
                        u64::try_from(event_ordinal)
                            .map_err(|_| corrupt("event version overflow"))?,
                    )
                    .and_then(|version| version.checked_add(1))
                    .ok_or_else(|| corrupt("event version overflow"))?;
                if !event.is_transactional
                    || event.recorded.stream_id() != &stream_id
                    || event.commit_id != commit
                    || event.operation_id != operation
                    || event.operation_fingerprint != fingerprint
                    || event.recorded.correlation_id() != Some(&correlation)
                    || event.recorded.causation_id() != cause.as_ref()
                    || event.recorded.actor() != content.actor.as_ref()
                    || event.recorded.stream_version().value() != version
                    || event.event_ordinal != event_ordinal
                    || event.event_count != count
                    || event.transaction_event_ordinal != ordinal
                    || event.transaction_event_count != event_count
                    || event.batch_id != decoded.batch_id
                {
                    return Err(corrupt(
                        "receipt does not cover its event with exact provenance",
                    ));
                }
                references.push(reference(config, &event.recorded, raw.time)?);
                events.push(event.recorded);
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| corrupt("event ordinal overflow"))?;
            }
        }
        streams.push(TransactionStreamReceipt::new(
            stream_id,
            base_version,
            events,
        ));
    }
    let mut integrity = TransactionReceipt::new(operation.clone(), fingerprint, streams)
        .with_bounded_context(config.bounded_context().clone())
        .with_correlation_id(correlation.clone());
    if let Some(cause) = cause {
        integrity = integrity.with_causation_id(cause);
    }
    if let Some(actor) = content.actor {
        integrity = integrity.with_actor(actor);
    }
    if let Some(scope) = content.security_scope {
        integrity = integrity.with_security_scope(
            rostfrei_core::SecurityScope::new(scope).map_err(|error| corrupt(error.to_string()))?,
        );
    }
    if let Some(completion) = content.completion {
        outcomes::validate_evidence_scope(config, completion.evidence.as_ref())?;
        integrity = integrity.with_completion(completion);
    }
    integrity.validate_integrity()?;
    if let Some(manifest) = content.manifest {
        let key = OutcomeKey::new(
            outcomes::historical_authority(config, journal.cached_info())?,
            config.bounded_context().clone(),
            operation,
        );
        let bytes = STANDARD
            .decode(manifest)
            .map_err(|error| corrupt(error.to_string()))?;
        let marker = OutcomeMarker::restore_manifest(&key, &bytes, integrity)?;
        outcomes::validate_evidence_scope(config, marker.evidence())?;
    }
    Ok(Some((correlation.as_str().to_owned(), references)))
}

struct Guard<'a> {
    stream_id: &'a StreamId,
    base_version: StreamVersion,
    operation: &'a OperationId,
    batch_id: &'a str,
    start: u64,
    ordinal: usize,
    index: usize,
}

// Guard verification uses its exact stored CAS checkpoint, never an aggregate
// replay. The checkpoint is validation data and does not become a flow reference.
async fn inspect_guard<F>(
    config: &NatsEventStoreConfig,
    journal: &jetstream::stream::Stream,
    guard: Guard<'_>,
    admit: &F,
) -> Result<bool, EventStoreError>
where
    F: Fn(usize) -> Result<bool, EventStoreError> + Sync,
{
    let sequence = guard
        .start
        .checked_add(u64::try_from(guard.ordinal).map_err(|_| corrupt("guard position overflow"))?)
        .ok_or_else(|| corrupt("guard position overflow"))?;
    let Some(raw) = read_raw(journal, sequence, admit).await? else {
        return Ok(false);
    };
    if raw.payload.len() > MAX_SUPPORTED_EVENT_BYTES {
        return Err(corrupt(
            "stored read guard exceeds the supported byte limit",
        ));
    }
    let headers = &raw.headers;
    if raw.subject.as_str()
        != config.transaction_guard_subject(guard.operation.as_str(), guard.index)
        || required_single_header(headers, "Content-Type")? != "application/json"
        || required_single_header(headers, "Nats-Batch-Id")? != guard.batch_id
        || required_single_header(headers, "Nats-Batch-Sequence")?
            .parse::<usize>()
            .ok()
            != guard.ordinal.checked_add(1)
        || optional_single_header(headers, "Nats-Expected-Stream")?
            != (guard.ordinal == 0).then_some(config.stream_name())
        || optional_single_header(headers, "Nats-Batch-Commit")?.is_some()
        || required_single_header(headers, NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT)?
            != config.event_subject(
                guard.stream_id.aggregate_type().as_str(),
                guard.stream_id.aggregate_id().as_str(),
            )
    {
        return Err(corrupt(
            "receipt read guard has incompatible atomic provenance",
        ));
    }
    let wire: TransactionGuardWire =
        serde_json::from_slice(&raw.payload).map_err(|error| corrupt(error.to_string()))?;
    if wire.operation_id != guard.operation.as_str()
        || stream_id_from_wire(wire.guarded_stream)? != *guard.stream_id
    {
        return Err(corrupt(
            "receipt read guard has incompatible participant identity",
        ));
    }
    let checkpoint = required_single_header(headers, "Nats-Expected-Last-Subject-Sequence")?
        .parse::<u64>()
        .map_err(|_| corrupt("invalid guard checkpoint sequence"))?;
    if guard.base_version == StreamVersion::ZERO {
        if checkpoint != 0 {
            return Err(corrupt("absent aggregate guard has a nonzero checkpoint"));
        }
    } else {
        if checkpoint == 0 || checkpoint >= guard.start {
            return Err(corrupt("guard checkpoint is outside prior history"));
        }
        let Some(raw) = read_raw(journal, checkpoint, admit).await? else {
            return Ok(false);
        };
        let event = decode_raw_event(config, &raw)?;
        if event.recorded.stream_id() != guard.stream_id
            || event.recorded.stream_version() != guard.base_version
        {
            return Err(corrupt(
                "guard checkpoint does not identify its base aggregate version",
            ));
        }
    }
    Ok(true)
}

async fn read_raw<F>(
    journal: &jetstream::stream::Stream,
    sequence: u64,
    admit: &F,
) -> Result<Option<jetstream::message::StreamMessage>, EventStoreError>
where
    F: Fn(usize) -> Result<bool, EventStoreError> + Sync,
{
    if sequence == 0 || sequence > journal.cached_info().state.last_sequence {
        return Err(corrupt("receipt reference is outside its captured journal"));
    }
    if !admit(0)? {
        return Ok(None);
    }
    // Official leader-served API. A follower Direct Get cannot prove absence at
    // the captured cutoff. Only known immutable sequence positions reach here.
    let raw = journal.get_raw_message(sequence).await.map_err(|error| {
        classify_required_raw_lookup_error(
            &error,
            "receipt references a missing journal record",
            "cannot read receipt journal record",
        )
    })?;
    if !admit(raw_examined_bytes(&raw)?)? {
        return Ok(None);
    }
    if raw.sequence != sequence {
        return Err(corrupt("raw lookup returned the wrong journal position"));
    }
    Ok(Some(raw))
}

pub fn raw_examined_bytes(
    raw: &jetstream::message::StreamMessage,
) -> Result<usize, EventStoreError> {
    let mut bytes = raw
        .payload
        .len()
        .checked_add(raw.subject.len())
        .and_then(|value| value.checked_add(512))
        .ok_or_else(|| corrupt("raw record size overflow"))?;
    for (name, values) in raw.headers.iter() {
        for value in values {
            bytes = bytes
                .checked_add(AsRef::<str>::as_ref(name).len())
                .and_then(|bytes| bytes.checked_add(value.as_str().len()))
                .and_then(|bytes| bytes.checked_add(4))
                .ok_or_else(|| corrupt("raw header size overflow"))?;
        }
    }
    Ok(bytes)
}

fn decode_raw_event(
    config: &NatsEventStoreConfig,
    raw: &jetstream::message::StreamMessage,
) -> Result<DecodedEvent, EventStoreError> {
    let event = decode_consumed_event(config, raw.subject.as_str(), &raw.headers, &raw.payload)?;
    if required_single_header(&raw.headers, "Nats-Msg-Id")? != event.recorded.event_id().as_str()
        || optional_single_header(&raw.headers, CORRELATION_ID_HEADER)?
            != event.recorded.correlation_id().map(CorrelationId::as_str)
    {
        return Err(corrupt("domain event headers disagree with its envelope"));
    }
    Ok(event)
}

fn reference(
    config: &NatsEventStoreConfig,
    event: &RecordedEvent,
    published: time::OffsetDateTime,
) -> Result<DomainEventRef, EventStoreError> {
    let milliseconds = u64::try_from(published.unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| corrupt("invalid domain publication timestamp"))?;
    let occurred_at = MessageTimestamp::from_unix_milliseconds(milliseconds)
        .map_err(|_| corrupt("invalid domain publication timestamp"))?;
    Ok(DomainEventRef::from_recorded(
        config.bounded_context().as_str(),
        event,
        occurred_at,
    ))
}
