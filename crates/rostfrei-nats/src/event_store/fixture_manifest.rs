//! Permanent fixture locators share the journal atomic batch, never command authority.
use super::*;

const SCHEMA_VERSION: u16 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredFixtureManifest {
    schema_version: u16,
    checksum: String,
    fixture: FixtureContent,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureContent {
    event_store_stream: String,
    application: String,
    bounded_context: String,
    stream: StreamIdentityWire,
    commit_id: String,
    operation_id: String,
    operation_fingerprint: String,
    base_stream_version: u64,
    event_count: usize,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_actor"
    )]
    actor: Option<rostfrei_core::ActorRef>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChecksumInput<'a> {
    schema_version: u16,
    fixture: &'a FixtureContent,
}

pub(super) struct DecodedFixtureManifest {
    pub(super) stream_id: StreamId,
    pub(super) commit_id: CommitId,
    pub(super) operation_id: OperationId,
    pub(super) operation_fingerprint: ContentFingerprint,
    pub(super) correlation_id: Option<CorrelationId>,
    pub(super) causation_id: Option<CausationId>,
    pub(super) actor: Option<rostfrei_core::ActorRef>,
    pub(super) base_version: StreamVersion,
    pub(super) event_count: usize,
    pub(super) batch_id: String,
    pub(super) batch_sequence: usize,
    pub(super) stream_sequence: u64,
    /// Half-open, contiguous journal range; its end is the terminal manifest.
    pub(super) event_sequences: Range<u64>,
}

impl DecodedFixtureManifest {
    pub(super) fn validate_commit(&self, commit: &StoredCommit) -> Result<(), EventStoreError> {
        if commit.fixture_batch_id.as_deref() != Some(self.batch_id.as_str())
            || commit.transaction.is_some()
            || commit.events.len() != self.event_count
            || commit.global_stream_sequences.len() != self.event_count
        {
            return Err(corrupt(
                "fixture commit does not match its terminal manifest",
            ));
        }
        for (ordinal, (event, sequence)) in commit
            .events
            .iter()
            .zip(&commit.global_stream_sequences)
            .enumerate()
        {
            self.validate_recorded_event(event, ordinal, *sequence)?;
        }
        Ok(())
    }

    pub(super) fn validate_event(
        &self,
        event: &DecodedEvent,
        sequence: u64,
    ) -> Result<(), EventStoreError> {
        if !event.is_fixture
            || event.is_transactional
            || event.event_count != self.event_count
            || event.batch_id != self.batch_id
        {
            return Err(corrupt(
                "fixture manifest has incompatible event batch coordinates",
            ));
        }
        self.validate_recorded_event(&event.recorded, event.event_ordinal, sequence)
    }

    fn validate_recorded_event(
        &self,
        event: &RecordedEvent,
        ordinal: usize,
        sequence: u64,
    ) -> Result<(), EventStoreError> {
        let ordinal = u64::try_from(ordinal).map_err(|_| corrupt("fixture ordinal overflow"))?;
        let expected_version = self
            .base_version
            .value()
            .checked_add(ordinal)
            .and_then(|value| value.checked_add(1));
        if event.stream_id() != &self.stream_id
            || self.batch_sequence
                != self
                    .event_count
                    .checked_add(1)
                    .ok_or_else(|| corrupt("fixture batch sequence overflow"))?
            || self.stream_sequence != self.event_sequences.end
            || event.commit_id() != &self.commit_id
            || event.operation_id() != &self.operation_id
            || event.operation_fingerprint() != self.operation_fingerprint
            || event.correlation_id() != self.correlation_id.as_ref()
            || event.causation_id() != self.causation_id.as_ref()
            || event.actor() != self.actor.as_ref()
            || expected_version != Some(event.stream_version().value())
            || self.event_sequences.start.checked_add(ordinal) != Some(sequence)
            || !self.event_sequences.contains(&sequence)
        {
            return Err(corrupt(
                "fixture manifest does not cover its event with exact provenance",
            ));
        }
        Ok(())
    }
}

fn checksum(schema_version: u16, fixture: &FixtureContent) -> Result<String, serde_json::Error> {
    Ok(encode_lower_hex(Sha256::digest(serde_json::to_vec(
        &ChecksumInput {
            schema_version,
            fixture,
        },
    )?)))
}

pub(super) fn encode_fixture_manifest(
    config: &NatsEventStoreConfig,
    stream: &StreamId,
    batch: &EventBatch,
    base_version: StreamVersion,
) -> Result<Vec<u8>, EventStoreError> {
    let fixture = FixtureContent {
        event_store_stream: config.stream_name().to_owned(),
        application: config.application().as_str().to_owned(),
        bounded_context: config.bounded_context().as_str().to_owned(),
        stream: stream_identity(stream),
        commit_id: batch.commit_id().as_str().to_owned(),
        operation_id: batch.operation_id().as_str().to_owned(),
        operation_fingerprint: batch.operation_fingerprint().to_hex(),
        base_stream_version: base_version.value(),
        event_count: batch.events().len(),
        correlation_id: batch.correlation_id().map(|id| id.as_str().to_owned()),
        causation_id: batch.causation_id().map(|id| id.as_str().to_owned()),
        actor: batch.actor().cloned(),
    };
    let checksum =
        checksum(SCHEMA_VERSION, &fixture).map_err(|error| invalid(error.to_string()))?;
    serde_json::to_vec(&StoredFixtureManifest {
        schema_version: SCHEMA_VERSION,
        checksum,
        fixture,
    })
    .map_err(|error| invalid(error.to_string()))
}

#[allow(clippy::too_many_lines)]
pub(super) fn decode_fixture_manifest(
    config: &NatsEventStoreConfig,
    subject: &str,
    stream_sequence: u64,
    headers: &HeaderMap,
    payload: &[u8],
) -> Result<DecodedFixtureManifest, EventStoreError> {
    if config.traffic_scope() != TrafficScope::Test || payload.len() > MAX_SUPPORTED_EVENT_BYTES {
        return Err(corrupt(
            "fixture manifest requires Test scope and supported payload size",
        ));
    }
    let wire: StoredFixtureManifest = serde_json::from_slice(payload)
        .map_err(|error| corrupt(format!("invalid fixture manifest: {error}")))?;
    if wire.schema_version != SCHEMA_VERSION
        || checksum(wire.schema_version, &wire.fixture)
            .map_err(|error| corrupt(error.to_string()))?
            != wire.checksum
    {
        return Err(corrupt("fixture manifest schema or checksum mismatch"));
    }
    let fixture = wire.fixture;
    if fixture.event_store_stream != config.stream_name()
        || fixture.application != config.application().as_str()
        || fixture.bounded_context != config.bounded_context().as_str()
    {
        return Err(corrupt("fixture manifest belongs to another event store"));
    }
    let stream_id = stream_id_from_wire(fixture.stream)?;
    let operation_id =
        OperationId::new(fixture.operation_id).map_err(|error| corrupt(error.to_string()))?;
    let commit_id = CommitId::new(fixture.commit_id).map_err(|error| corrupt(error.to_string()))?;
    let operation_fingerprint = ContentFingerprint::from_hex(&fixture.operation_fingerprint)
        .map_err(|error| corrupt(error.to_string()))?;
    let correlation_id = fixture
        .correlation_id
        .map(CorrelationId::new)
        .transpose()
        .map_err(|error| corrupt(error.to_string()))?;
    let causation_id = fixture
        .causation_id
        .map(CausationId::new)
        .transpose()
        .map_err(|error| corrupt(error.to_string()))?;
    if commit_id != derive_commit_id(&stream_id, &operation_id)
        || subject
            != config.fixture_receipt_subject(
                &stream_id,
                operation_id.as_str(),
                correlation_id.as_ref().map(CorrelationId::as_str),
                causation_id.as_ref().map(CausationId::as_str),
            )
    {
        return Err(corrupt("fixture manifest identity or subject mismatch"));
    }
    let batch_id = required_single_header(headers, "Nats-Batch-Id")?;
    let batch_sequence = required_single_header(headers, "Nats-Batch-Sequence")?
        .parse::<usize>()
        .map_err(|_| corrupt("invalid fixture batch sequence"))?;
    if fixture.event_count == 0
        || fixture.event_count > MAX_EVENTS_PER_BATCH
        || fixture.event_count.checked_add(1) != Some(batch_sequence)
        || batch_id.is_empty()
        || batch_id.len() > 64
        || required_single_header(headers, "Content-Type")? != "application/json"
        || required_single_header(headers, "Nats-Batch-Commit")? != NATS_BATCH_COMMIT_FINAL
        || required_single_header(headers, "Nats-Expected-Last-Subject-Sequence")? != "0"
        || required_single_header(headers, NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT)?
            != config.fixture_receipt_subject_filter(&stream_id, operation_id.as_str())
        || optional_single_header(headers, "Nats-Expected-Stream")?.is_some()
        || optional_single_header(headers, "Nats-Msg-Id")?.is_some()
        || optional_single_header(headers, CORRELATION_ID_HEADER)?
            != correlation_id.as_ref().map(CorrelationId::as_str)
    {
        return Err(corrupt("fixture manifest has incompatible atomic headers"));
    }
    let count =
        u64::try_from(fixture.event_count).map_err(|_| corrupt("fixture event count overflow"))?;
    let start = stream_sequence
        .checked_sub(count)
        .filter(|sequence| *sequence != 0)
        .ok_or_else(|| corrupt("fixture range underflow"))?;
    fixture
        .base_stream_version
        .checked_add(count)
        .ok_or_else(|| corrupt("fixture version overflow"))?;
    Ok(DecodedFixtureManifest {
        stream_id,
        commit_id,
        operation_id,
        operation_fingerprint,
        correlation_id,
        causation_id,
        actor: fixture.actor,
        base_version: StreamVersion::new(fixture.base_stream_version),
        event_count: fixture.event_count,
        batch_id: batch_id.to_owned(),
        batch_sequence,
        stream_sequence,
        event_sequences: start..stream_sequence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one manifest fixture exercises subject, atomic coordinates, checksum, and attribution tampering"
    )]
    fn fixture_manifest_binds_subject_atomic_coordinates_and_event_attribution() {
        let context = rostfrei_messaging_core::ApplicationName::new("manifest-test")
            .unwrap()
            .test_bounded_context("orders")
            .unwrap();
        let config = NatsEventStoreConfig::for_bounded_context(&context).unwrap();
        let stream = StreamId::new(
            AggregateType::new("Order").unwrap(),
            AggregateId::new("one").unwrap(),
        );
        let operation = OperationId::new("seed").unwrap();
        let commit = derive_commit_id(&stream, &operation);
        let batch = EventBatch::new(
            commit.clone(),
            operation,
            ContentFingerprint::digest("seed"),
            vec![NewEvent::new(derive_event_id(&commit, 0), "created", 1, b"{}".to_vec()).unwrap()],
        )
        .unwrap()
        .with_correlation_id(CorrelationId::new("flow.*").unwrap())
        .with_actor(rostfrei_core::ActorRef::new("people", "alice").unwrap());
        let recorded = record_batch(&stream, StreamVersion::ZERO, &batch).unwrap();
        let event_payload =
            encode_events_inner(&config, &stream, &batch, recorded.events(), None, true)
                .unwrap()
                .remove(0);
        let mut event_headers = HeaderMap::new();
        for (name, value) in [
            ("Content-Type", "application/json"),
            ("Nats-Batch-Id", "fixture-batch"),
            ("Nats-Batch-Sequence", "1"),
            ("Nats-Expected-Last-Subject-Sequence", "0"),
            (CORRELATION_ID_HEADER, "flow.*"),
        ] {
            event_headers.insert(name, value);
        }
        event_headers.insert("Nats-Expected-Stream", config.stream_name());
        event_headers.insert(
            NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT,
            config.event_subject("Order", "one"),
        );
        let event = decode_consumed_event(
            &config,
            &config.event_subject("Order", "one"),
            &event_headers,
            &event_payload,
        )
        .unwrap();
        let subject = config.fixture_receipt_subject(&stream, "seed", Some("flow.*"), None);
        let payload =
            encode_fixture_manifest(&config, &stream, &batch, StreamVersion::ZERO).unwrap();
        let mut headers = HeaderMap::new();
        for (name, values) in event_headers.iter() {
            if AsRef::<str>::as_ref(name) != "Nats-Expected-Stream" {
                for value in values {
                    headers.append(name.clone(), value.clone());
                }
            }
        }
        headers.insert("Nats-Batch-Sequence", "2");
        headers.insert("Nats-Batch-Commit", "1");
        headers.insert(
            NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT,
            config.fixture_receipt_subject_filter(&stream, "seed"),
        );
        let manifest = decode_fixture_manifest(&config, &subject, 2, &headers, &payload).unwrap();
        manifest.validate_event(&event, 1).unwrap();
        assert!(manifest.validate_event(&event, 2).is_err());
        assert!(decode_fixture_manifest(&config, &subject, 1, &headers, &payload).is_err());
        assert!(
            decode_fixture_manifest(
                &config,
                &config.fixture_receipt_subject(&stream, "seed", Some("other"), None),
                2,
                &headers,
                &payload
            )
            .is_err()
        );
        let mut narrow = headers.clone();
        narrow.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT, subject.clone());
        assert!(decode_fixture_manifest(&config, &subject, 2, &narrow, &payload).is_err());

        // A well-checksummed replacement still cannot attribute the recorded event
        // to another actor or borrow an unrelated native atomic batch.
        let mut forged: StoredFixtureManifest = serde_json::from_slice(&payload).unwrap();
        forged.fixture.actor = Some(rostfrei_core::ActorRef::new("people", "bob").unwrap());
        assert!(
            decode_fixture_manifest(
                &config,
                &subject,
                2,
                &headers,
                &serde_json::to_vec(&forged).unwrap()
            )
            .is_err()
        );
        forged.checksum = checksum(forged.schema_version, &forged.fixture).unwrap();
        let forged = decode_fixture_manifest(
            &config,
            &subject,
            2,
            &headers,
            &serde_json::to_vec(&forged).unwrap(),
        )
        .unwrap();
        assert!(forged.validate_event(&event, 1).is_err());
        headers.insert("Nats-Batch-Id", "another-batch");
        let unrelated = decode_fixture_manifest(&config, &subject, 2, &headers, &payload).unwrap();
        assert!(unrelated.validate_event(&event, 1).is_err());
    }
}
