#[path = "../src/event_store.rs"]
mod event_store;
#[path = "../src/event_store_config.rs"]
mod event_store_config;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_nats::jetstream::message::PublishMessage;
use event_store::{provision_event_store, NatsEventStore};
use event_store_config::NatsEventStoreConfig;
use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreErrorKind, ExecutionMetadata, ExpectedVersion, NewEvent, OperationId, StreamId,
    StreamVersion,
};
use rostfrei_testing::event_store_contract;

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
#[ignore = "requires a real NATS server configured by ROSTFREI_NATS_URL"]
#[allow(clippy::too_many_lines)]
async fn real_nats_event_store_contract_and_operator_policy() {
    let Ok(url) = std::env::var("ROSTFREI_NATS_URL") else {
        eprintln!("ROSTFREI_NATS_URL is not set; skipping real NATS integration test");
        return;
    };
    let context = connect_context(&url).await;
    let (stream_name, subject_prefix) = unique_names("contract");
    let config = NatsEventStoreConfig::new(
        stream_name,
        subject_prefix,
        64 * 1024 * 1024,
        2 * 1024 * 1024,
        1,
        Duration::from_secs(5),
    )
    .expect("valid integration config");

    let missing = NatsEventStore::connect(context.clone(), config.clone()).await;
    assert!(
        matches!(missing, Err(ref error) if error.kind() == EventStoreErrorKind::Unavailable),
        "connect must not provision a missing stream"
    );
    provision_event_store(&context, &config)
        .await
        .expect("explicit provisioning should succeed");
    provision_event_store(&context, &config)
        .await
        .expect("repeated event-store provisioning must be idempotent");
    let store = NatsEventStore::connect(context.clone(), config.clone())
        .await
        .expect("provisioned stream should connect");
    assert_eq!(store.config(), &config);

    event_store_contract::run(|| store.clone()).await;

    let concurrent_stream = stream("concurrent-atomic-batches");
    let (first_result, second_result) = tokio::join!(
        store.append(
            &concurrent_stream,
            ExpectedVersion::NoStream,
            batch(
                &concurrent_stream,
                "concurrent-atomic-a",
                "concurrent-a",
                &[b"a-1", b"a-2", b"a-3"],
            ),
        ),
        store.append(
            &concurrent_stream,
            ExpectedVersion::NoStream,
            batch(
                &concurrent_stream,
                "concurrent-atomic-b",
                "concurrent-b",
                &[b"b-1", b"b-2", b"b-3"],
            ),
        ),
    );
    let concurrent_results = [first_result, second_result];
    assert_eq!(
        concurrent_results
            .iter()
            .filter(|result| result.is_ok())
            .count(),
        1
    );
    assert_eq!(
        concurrent_results
            .iter()
            .filter(|result| matches!(
                result,
                Err(error) if error.kind() == EventStoreErrorKind::Conflict
            ))
            .count(),
        1
    );
    let concurrent_history = store
        .load(&concurrent_stream)
        .await
        .expect("winning atomic batch should load");
    assert_eq!(concurrent_history.len(), 3);
    assert!(concurrent_history
        .windows(2)
        .all(|events| events[0].commit_id() == events[1].commit_id()));

    let restart_stream = stream("restart-retry");
    let retried_batch = batch(
        &restart_stream,
        "restart-operation",
        "restart-content",
        &[b"first", b"second"],
    );
    let first = store
        .append(
            &restart_stream,
            ExpectedVersion::NoStream,
            retried_batch.clone(),
        )
        .await
        .expect("initial restart-retry append should succeed");
    store
        .append(
            &restart_stream,
            ExpectedVersion::Exact(StreamVersion::new(2)),
            batch(
                &restart_stream,
                "restart-later-operation",
                "restart-later-content",
                &[b"later"],
            ),
        )
        .await
        .expect("later append should succeed");

    let reconnected = NatsEventStore::connect(connect_context(&url).await, config.clone())
        .await
        .expect("a new adapter should connect without provisioning");
    let replay = reconnected
        .append(&restart_stream, ExpectedVersion::NoStream, retried_batch)
        .await
        .expect("exact retry should survive adapter and client restart");
    assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
    assert_eq!(replay.events(), first.events());

    let mut stream_info = context
        .get_stream(config.stream_name())
        .await
        .expect("stream info should be available");
    let messages_before = stream_info
        .info()
        .await
        .expect("stream info should refresh")
        .state
        .messages;
    let atomic_stream = stream("one-wire-commit");
    store
        .append(
            &atomic_stream,
            ExpectedVersion::NoStream,
            batch(
                &atomic_stream,
                "one-wire-operation",
                "one-wire-content",
                &[b"one", b"two", b"three"],
            ),
        )
        .await
        .expect("atomic batch should append");
    let messages_after = stream_info
        .info()
        .await
        .expect("stream info should refresh")
        .state
        .messages;
    assert_eq!(
        messages_after,
        messages_before + 3,
        "each domain event must use one JetStream message"
    );
    let atomic_subject = config.aggregate_subject(
        atomic_stream.aggregate_type().as_str(),
        atomic_stream.aggregate_id().as_str(),
    );
    let mut next_sequence = 1_u64;
    let mut batch_id = None;
    for (index, expected_payload) in [b"one".as_slice(), b"two", b"three"].iter().enumerate() {
        let expected_batch_sequence = (index + 1).to_string();
        let expected_payload_base64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, expected_payload);
        let stored_event = stream_info
            .get_first_raw_message_by_subject(&atomic_subject, next_sequence)
            .await
            .expect("stored atomic event");
        next_sequence = stored_event.sequence + 1;
        assert_eq!(
            stored_event
                .headers
                .get("Content-Type")
                .map(async_nats::HeaderValue::as_str),
            Some("application/json")
        );
        assert_eq!(
            stored_event
                .headers
                .get("Nats-Batch-Sequence")
                .map(async_nats::HeaderValue::as_str),
            Some(expected_batch_sequence.as_str())
        );
        assert_eq!(
            stored_event
                .headers
                .get("Nats-Expected-Stream")
                .map(async_nats::HeaderValue::as_str),
            (index == 0).then_some(config.stream_name())
        );
        assert_eq!(
            stored_event
                .headers
                .get("Nats-Expected-Last-Subject-Sequence")
                .map(async_nats::HeaderValue::as_str),
            (index == 0).then_some("0")
        );
        let stored_batch_id = stored_event
            .headers
            .get("Nats-Batch-Id")
            .expect("stored event has a batch identity")
            .as_str();
        if let Some(batch_id) = &batch_id {
            assert_eq!(stored_batch_id, batch_id);
        } else {
            batch_id = Some(stored_batch_id.to_owned());
        }
        assert_eq!(
            stored_event
                .headers
                .get("Nats-Batch-Commit")
                .map(async_nats::HeaderValue::as_str),
            (index == 2).then_some("1")
        );
        let wire: serde_json::Value =
            serde_json::from_slice(&stored_event.payload).expect("stored event should be JSON");
        assert!(wire.get("events").is_none());
        assert_eq!(
            wire.pointer("/event/payloadBase64")
                .and_then(serde_json::Value::as_str),
            Some(expected_payload_base64.as_str())
        );
    }

    let boundary_stream = stream("maximum-atomic-batch");
    let boundary_outcome = store
        .append(
            &boundary_stream,
            ExpectedVersion::NoStream,
            repeated_batch(
                &boundary_stream,
                "maximum-batch-operation",
                "maximum-batch-content",
                1_000,
            ),
        )
        .await
        .expect("the ADR-50 maximum event count should append atomically");
    assert_eq!(boundary_outcome.events().len(), 1_000);
    assert_eq!(
        store
            .load(&boundary_stream)
            .await
            .expect("maximum atomic batch should load")
            .len(),
        1_000
    );

    let incompatible_stream = stream("incompatible-wire");
    let incompatible_subject = config.aggregate_subject(
        incompatible_stream.aggregate_type().as_str(),
        incompatible_stream.aggregate_id().as_str(),
    );
    context
        .send_publish(
            incompatible_subject,
            PublishMessage::build()
                .payload(br#"{"schemaVersion":99}"#.to_vec().into())
                .expected_stream(config.stream_name()),
        )
        .await
        .expect("send incompatible commit")
        .await
        .expect("store incompatible commit for corruption test");
    let corrupt = store.load(&incompatible_stream).await;
    assert!(matches!(
        corrupt,
        Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory
    ));

    let mismatch = NatsEventStoreConfig::new(
        config.stream_name(),
        config.subject_prefix(),
        config.max_stream_bytes() + 1,
        config.max_event_bytes(),
        config.replicas(),
        config.puback_timeout(),
    )
    .expect("valid mismatching config");
    let mismatch_result = NatsEventStore::connect(context.clone(), mismatch).await;
    assert!(matches!(
        mismatch_result,
        Err(ref error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch
    ));

    capacity_is_reported_distinctly(&context).await;
}

async fn capacity_is_reported_distinctly(context: &async_nats::jetstream::Context) {
    let (stream_name, subject_prefix) = unique_names("capacity");
    let config = NatsEventStoreConfig::new(
        stream_name,
        subject_prefix,
        4096,
        2048,
        1,
        Duration::from_secs(5),
    )
    .expect("valid capacity config");
    provision_event_store(context, &config)
        .await
        .expect("capacity stream should provision");
    let store = NatsEventStore::connect(context.clone(), config)
        .await
        .expect("capacity stream should connect");
    let capacity_stream = stream("capacity");
    store
        .append(
            &capacity_stream,
            ExpectedVersion::NoStream,
            owned_payload_batch(
                &capacity_stream,
                "capacity-first-operation",
                "capacity-first-content",
                vec![1; 700],
            ),
        )
        .await
        .expect("capacity stream should accept an initial event");
    let before = store
        .load(&capacity_stream)
        .await
        .expect("initial capacity history");
    let large_payload = vec![2; 700];
    let result = store
        .append(
            &capacity_stream,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            batch(
                &capacity_stream,
                "capacity-atomic-operation",
                "capacity-atomic-content",
                &[
                    large_payload.as_slice(),
                    large_payload.as_slice(),
                    large_payload.as_slice(),
                ],
            ),
        )
        .await;
    assert!(matches!(
        result,
        Err(ref error) if error.kind() == EventStoreErrorKind::CapacityExhausted
    ));
    assert_eq!(
        store
            .load(&capacity_stream)
            .await
            .expect("capacity failure history"),
        before,
        "capacity failure must not store an event prefix"
    );
}

async fn connect_context(url: &str) -> async_nats::jetstream::Context {
    let client = async_nats::connect(url)
        .await
        .expect("NATS connection should succeed");
    async_nats::jetstream::new(client)
}

fn unique_names(label: &str) -> (String, String) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should follow the Unix epoch")
        .as_nanos();
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let process = std::process::id();
    (
        format!("EVENT_STORE_{label}_{process}_{nanos}_{counter}").to_ascii_uppercase(),
        format!("private.event-store-test.{label}.{process}.{nanos}.{counter}"),
    )
}

fn stream(id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new("IntegrationAggregate").expect("valid aggregate type"),
        AggregateId::new(id).expect("valid aggregate id"),
    )
}

fn batch(
    stream: &StreamId,
    operation_id: &str,
    fingerprint_content: &str,
    payloads: &[&[u8]],
) -> EventBatch {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation_id).expect("valid operation id"),
        ContentFingerprint::digest(fingerprint_content),
    );
    let events = payloads
        .iter()
        .enumerate()
        .map(|(ordinal, payload)| {
            NewEvent::new(
                metadata.event_id(u32::try_from(ordinal).expect("small integration batch")),
                "integration-event",
                1,
                payload.to_vec(),
            )
            .expect("valid integration event")
        })
        .collect();
    EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )
    .expect("non-empty integration batch")
}

fn owned_payload_batch(
    stream: &StreamId,
    operation_id: &str,
    fingerprint_content: &str,
    payload: Vec<u8>,
) -> EventBatch {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation_id).expect("valid operation id"),
        ContentFingerprint::digest(fingerprint_content),
    );
    EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![
            NewEvent::new(metadata.event_id(0), "integration-event", 1, payload)
                .expect("valid integration event"),
        ],
    )
    .expect("non-empty integration batch")
}

fn repeated_batch(
    stream: &StreamId,
    operation_id: &str,
    fingerprint_content: &str,
    event_count: u32,
) -> EventBatch {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation_id).expect("valid operation id"),
        ContentFingerprint::digest(fingerprint_content),
    );
    let events = (0..event_count)
        .map(|ordinal| {
            NewEvent::new(
                metadata.event_id(ordinal),
                "integration-event",
                1,
                vec![u8::try_from(ordinal % 251).expect("bounded event byte")],
            )
            .expect("valid integration event")
        })
        .collect();
    EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )
    .expect("non-empty integration batch")
}
