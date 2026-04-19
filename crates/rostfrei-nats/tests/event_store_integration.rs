#[path = "../src/event_store.rs"]
mod event_store;
#[path = "../src/event_store_config.rs"]
mod event_store_config;
#[path = "../src/hex.rs"]
mod hex;
#[path = "../src/stream_policy.rs"]
mod stream_policy;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_nats::jetstream::message::PublishMessage;
use event_store::{NatsEventStore, provision_event_store};
use event_store_config::NatsEventStoreConfig;
use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreError, EventStoreErrorKind, ExecutionMetadata, ExpectedVersion, NewEvent,
    OperationId, RecordedEvent, StreamId, StreamVersion,
};
use rostfrei_messaging_core::{ApplicationName, BoundedContext};
use rostfrei_testing::event_store_contract;

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn checked_add_u64(value: u64, increment: u64, context: &'static str) -> TestResult<u64> {
    value
        .checked_add(increment)
        .ok_or_else(|| format!("{context} exceeds u64").into())
}

fn checked_add_usize(value: usize, increment: usize, context: &'static str) -> TestResult<usize> {
    value
        .checked_add(increment)
        .ok_or_else(|| format!("{context} exceeds usize").into())
}

fn checked_add_i64(value: i64, increment: i64, context: &'static str) -> TestResult<i64> {
    value
        .checked_add(increment)
        .ok_or_else(|| format!("{context} exceeds i64").into())
}

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
#[ignore = "requires a real NATS server configured by ROSTFREI_NATS_URL"]
#[allow(clippy::too_many_lines)]
async fn real_nats_event_store_contract_and_operator_policy() {
    let Ok(url) = std::env::var("ROSTFREI_NATS_URL") else {
        eprintln!("ROSTFREI_NATS_URL is not set; skipping real NATS integration test");
        return;
    };
    let context = connect_context(&url)
        .await
        .expect("real NATS JetStream context");
    let (bounded_context, stream_name) = unique_names("contract").expect("unique contract names");
    let config = NatsEventStoreConfig::new(&bounded_context, stream_name)
        .expect("valid integration config")
        .with_storage_limits(64 * 1024 * 1024, 2 * 1024 * 1024)
        .expect("valid integration storage limits");

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

    event_store_contract::try_run(|| store.clone())
        .await
        .expect("event-store contract");

    let concurrent_stream = stream("concurrent-atomic-batches").expect("concurrent stream id");
    let concurrent_results: [_; 2] = tokio::join!(
        store.append(
            &concurrent_stream,
            ExpectedVersion::NoStream,
            batch(
                &concurrent_stream,
                "concurrent-atomic-a",
                "concurrent-a",
                &[b"a-1", b"a-2", b"a-3"],
            )
            .expect("first concurrent batch"),
        ),
        store.append(
            &concurrent_stream,
            ExpectedVersion::NoStream,
            batch(
                &concurrent_stream,
                "concurrent-atomic-b",
                "concurrent-b",
                &[b"b-1", b"b-2", b"b-3"],
            )
            .expect("second concurrent batch"),
        ),
    )
    .into();
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
    assert!(
        concurrent_history
            .windows(2)
            .all(|events| events[0].commit_id() == events[1].commit_id())
    );

    let restart_stream = stream("restart-retry").expect("restart stream id");
    let retried_batch = batch(
        &restart_stream,
        "restart-operation",
        "restart-content",
        &[b"first", b"second"],
    )
    .expect("restart retry batch");
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
            )
            .expect("later restart batch"),
        )
        .await
        .expect("later append should succeed");

    let reconnected = NatsEventStore::connect(
        connect_context(&url)
            .await
            .expect("reconnected NATS JetStream context"),
        config.clone(),
    )
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
    let atomic_stream = stream("one-wire-commit").expect("one-wire stream id");
    store
        .append(
            &atomic_stream,
            ExpectedVersion::NoStream,
            batch(
                &atomic_stream,
                "one-wire-operation",
                "one-wire-content",
                &[b"one", b"two", b"three"],
            )
            .expect("one-wire atomic batch"),
        )
        .await
        .expect("atomic batch should append");
    let messages_after = stream_info
        .info()
        .await
        .expect("stream info should refresh")
        .state
        .messages;
    let expected_messages_after = checked_add_u64(
        messages_before,
        3,
        "expected JetStream message count after atomic append",
    )
    .expect("expected message count arithmetic");
    assert_eq!(
        messages_after, expected_messages_after,
        "each domain event must use one JetStream message"
    );
    let atomic_subject = config.aggregate_subject(
        atomic_stream.aggregate_type().as_str(),
        atomic_stream.aggregate_id().as_str(),
    );
    let mut next_sequence = 1_u64;
    let mut batch_id = None;
    for (index, expected_payload) in [b"one".as_slice(), b"two", b"three"].iter().enumerate() {
        let expected_batch_sequence =
            checked_add_usize(index, 1, "expected one-based batch sequence")
                .expect("expected batch sequence arithmetic")
                .to_string();
        let expected_payload_base64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, expected_payload);
        let stored_event = stream_info
            .get_first_raw_message_by_subject(&atomic_subject, next_sequence)
            .await
            .expect("stored atomic event");
        next_sequence = checked_add_u64(
            stored_event.sequence,
            1,
            "next JetStream sequence after stored atomic event",
        )
        .expect("next JetStream sequence arithmetic");
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
            wire.pointer("/schemaVersion")
                .and_then(serde_json::Value::as_u64),
            Some(3)
        );
        assert_eq!(
            wire.pointer("/event/application")
                .and_then(serde_json::Value::as_str),
            Some(config.application().as_str())
        );
        assert_eq!(
            wire.pointer("/event/boundedContext")
                .and_then(serde_json::Value::as_str),
            Some(config.bounded_context().as_str())
        );
        assert_eq!(
            wire.pointer("/event/payloadBase64")
                .and_then(serde_json::Value::as_str),
            Some(expected_payload_base64.as_str())
        );
    }

    let boundary_stream = stream("maximum-atomic-batch").expect("maximum batch stream id");
    let boundary_outcome = store
        .append(
            &boundary_stream,
            ExpectedVersion::NoStream,
            repeated_batch(
                &boundary_stream,
                "maximum-batch-operation",
                "maximum-batch-content",
                1_000,
            )
            .expect("maximum atomic batch fixture"),
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

    let incompatible_stream = stream("incompatible-wire").expect("incompatible stream id");
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
        &BoundedContext::new(
            config.application().clone(),
            config.bounded_context().clone(),
        ),
        config.stream_name(),
    )
    .expect("valid mismatching config")
    .with_storage_limits(
        checked_add_i64(
            config.max_stream_bytes(),
            1,
            "mismatching maximum stream byte limit",
        )
        .expect("mismatching stream limit arithmetic"),
        config.max_event_bytes(),
    )
    .expect("valid mismatching storage limits")
    .with_replicas(config.replicas())
    .expect("valid matching replicas")
    .with_puback_timeout(config.puback_timeout())
    .expect("valid matching PubAck timeout");
    let mismatch_result = NatsEventStore::connect(context.clone(), mismatch).await;
    assert!(matches!(
        mismatch_result,
        Err(ref error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch
    ));

    let capacity = capacity_observations(&context)
        .await
        .expect("capacity observations");
    assert!(matches!(
        &capacity.append_result,
        Err(error) if error.kind() == EventStoreErrorKind::CapacityExhausted
    ));
    assert_eq!(
        capacity.history_after, capacity.history_before,
        "capacity failure must not store an event prefix"
    );
}

struct CapacityObservations {
    append_result: Result<AppendOutcome, EventStoreError>,
    history_before: Vec<RecordedEvent>,
    history_after: Vec<RecordedEvent>,
}

async fn capacity_observations(
    context: &async_nats::jetstream::Context,
) -> TestResult<CapacityObservations> {
    let (bounded_context, stream_name) = unique_names("capacity")?;
    let config = NatsEventStoreConfig::new(&bounded_context, stream_name)?
        .with_storage_limits(4096, 2048)?;
    provision_event_store(context, &config).await?;
    let store = NatsEventStore::connect(context.clone(), config).await?;
    let capacity_stream = stream("capacity")?;
    store
        .append(
            &capacity_stream,
            ExpectedVersion::NoStream,
            owned_payload_batch(
                &capacity_stream,
                "capacity-first-operation",
                "capacity-first-content",
                vec![1; 700],
            )?,
        )
        .await?;
    let before = store.load(&capacity_stream).await?;
    let large_payload = vec![2; 700];
    let append_result = store
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
            )?,
        )
        .await;
    let history_after = store.load(&capacity_stream).await?;
    Ok(CapacityObservations {
        append_result,
        history_before: before,
        history_after,
    })
}

async fn connect_context(url: &str) -> TestResult<async_nats::jetstream::Context> {
    let client = async_nats::connect(url).await?;
    Ok(async_nats::jetstream::new(client))
}

fn unique_names(label: &str) -> TestResult<(BoundedContext, String)> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let process = std::process::id();
    Ok((
        ApplicationName::new(format!("rostfrei-test-{process}-{counter}"))?
            .bounded_context(label)?,
        format!("EVENT_STORE_{label}_{process}_{nanos}_{counter}").to_ascii_uppercase(),
    ))
}

fn stream(id: &str) -> TestResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new("IntegrationAggregate")?,
        AggregateId::new(id)?,
    ))
}

fn batch(
    stream: &StreamId,
    operation_id: &str,
    fingerprint_content: &str,
    payloads: &[&[u8]],
) -> TestResult<EventBatch> {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation_id)?,
        ContentFingerprint::digest(fingerprint_content),
    );
    let events = payloads
        .iter()
        .enumerate()
        .map(|(ordinal, payload)| {
            Ok(NewEvent::new(
                metadata.event_id(u32::try_from(ordinal)?),
                "integration-event",
                1,
                payload.to_vec(),
            )?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    Ok(EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )?)
}

fn owned_payload_batch(
    stream: &StreamId,
    operation_id: &str,
    fingerprint_content: &str,
    payload: Vec<u8>,
) -> TestResult<EventBatch> {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation_id)?,
        ContentFingerprint::digest(fingerprint_content),
    );
    Ok(EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![NewEvent::new(
            metadata.event_id(0),
            "integration-event",
            1,
            payload,
        )?],
    )?)
}

fn repeated_batch(
    stream: &StreamId,
    operation_id: &str,
    fingerprint_content: &str,
    event_count: u32,
) -> TestResult<EventBatch> {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation_id)?,
        ContentFingerprint::digest(fingerprint_content),
    );
    let events = (0..event_count)
        .map(|ordinal| {
            Ok(NewEvent::new(
                metadata.event_id(ordinal),
                "integration-event",
                1,
                vec![u8::try_from(ordinal % 251)?],
            )?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    Ok(EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )?)
}
