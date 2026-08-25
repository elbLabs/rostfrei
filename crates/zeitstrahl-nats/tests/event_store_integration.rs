#[path = "../src/event_store.rs"]
mod event_store;
#[path = "../src/event_store_config.rs"]
mod event_store_config;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_nats::jetstream::message::PublishMessage;
use event_store::{provision_event_store, NatsEventStore};
use event_store_config::NatsEventStoreConfig;
use zeitstrahl_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreErrorKind, ExecutionMetadata, ExpectedVersion, NewEvent, OperationId, StreamId,
    StreamVersion,
};
use zeitstrahl_testing::event_store_contract;

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
#[ignore = "requires a real NATS server configured by ZEITSTRAHL_NATS_URL"]
#[allow(clippy::too_many_lines)]
async fn real_nats_event_store_contract_and_operator_policy() {
    let Ok(url) = std::env::var("ZEITSTRAHL_NATS_URL") else {
        eprintln!("ZEITSTRAHL_NATS_URL is not set; skipping real NATS integration test");
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
        messages_before + 1,
        "one aggregate batch must use one JetStream message"
    );
    let atomic_subject = config.aggregate_subject(
        atomic_stream.aggregate_type().as_str(),
        atomic_stream.aggregate_id().as_str(),
    );
    let atomic_commit = stream_info
        .get_last_raw_message_by_subject(&atomic_subject)
        .await
        .expect("stored atomic commit");
    assert_eq!(
        atomic_commit
            .headers
            .get("Content-Type")
            .map(async_nats::HeaderValue::as_str),
        Some("application/json")
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
        config.max_commit_bytes(),
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
    let mut version = StreamVersion::ZERO;
    let mut appended = 0_usize;

    for ordinal in 0..16_u64 {
        let operation = format!("capacity-operation-{ordinal}");
        let fingerprint_seed = format!("capacity-content-{ordinal}");
        let expected = if version == StreamVersion::ZERO {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(version)
        };
        match store
            .append(
                &capacity_stream,
                expected,
                owned_payload_batch(
                    &capacity_stream,
                    &operation,
                    &fingerprint_seed,
                    vec![u8::try_from(ordinal).expect("small ordinal"); 700],
                ),
            )
            .await
        {
            Ok(AppendOutcome::Appended(events)) => {
                version = events.last().expect("append has an event").stream_version();
                appended += 1;
            }
            Err(error) if error.kind() == EventStoreErrorKind::CapacityExhausted => {
                assert!(appended > 0, "capacity stream should accept a commit first");
                return;
            }
            result => panic!("expected configured capacity exhaustion, got {result:?}"),
        }
    }
    panic!("small DiscardNew stream did not exhaust its configured capacity");
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
