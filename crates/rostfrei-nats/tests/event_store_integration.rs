#[path = "../src/event_store.rs"]
mod event_store;
#[path = "../src/event_store_config.rs"]
mod event_store_config;
#[path = "../src/hex.rs"]
mod hex;
#[path = "../src/stream_policy.rs"]
mod stream_policy;

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use async_nats::{Request, jetstream::message::PublishMessage};
use event_store::{NatsEventStore, provision_event_store};
use event_store_config::{
    DEFAULT_EVENT_STORE_MAX_EVENT_BYTES, MAX_SUPPORTED_EVENT_BYTES, NatsEventStoreConfig,
};
use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreError, EventStoreErrorKind, EventTransaction, ExecutionMetadata, ExpectedVersion,
    MAX_TRANSACTION_ITEMS, NewEvent, OperationId, RecordedEvent, StreamId, StreamVersion,
    TransactionAppendOutcome, TransactionParticipant,
};
use rostfrei_messaging_core::{ApplicationName, BoundedContext};
use rostfrei_testing::event_store_contract;
use sha2::{Digest, Sha256};
use tokio::sync::Barrier;

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

fn check(condition: bool, context: &'static str) -> TestResult<()> {
    if condition {
        Ok(())
    } else {
        Err(context.into())
    }
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
    legacy_history_remains_readable_after_lower_limit_provisioning(&context)
        .await
        .expect("legacy history migration coverage");
    transaction_history_remains_readable_after_lower_limit_provisioning(&context)
        .await
        .expect("transaction history migration coverage");
    legacy_event_store_policy_is_upgraded(&context)
        .await
        .expect("legacy stream policy migration coverage");
    max_payload_is_validated_before_provisioning(&context)
        .await
        .expect("maximum payload validation coverage");
    let (bounded_context, stream_name) = unique_names("contract").expect("unique contract names");
    let config = NatsEventStoreConfig::new(&bounded_context, stream_name)
        .expect("valid integration config")
        .with_storage_limits(64 * 1024 * 1024, 512 * 1024)
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

    let (event_store_contract_result, transaction_contract_result) = tokio::join!(
        event_store_contract::try_run(|| store.clone()),
        event_store_contract::try_run_atomic_multi_stream_transactions(|| store.clone()),
    );
    event_store_contract_result.expect("event-store contract");
    transaction_contract_result.expect("atomic multi-stream transaction contract");
    transaction_contract_and_wire_policy(&store, &context, &config)
        .await
        .expect("transaction contract and wire policy");
    concurrent_identical_transaction_reloads_receipt(&store)
        .await
        .expect("concurrent identical transaction receipt reload");
    concurrent_transaction_identity_race(&store)
        .await
        .expect("transaction identity race");
    unavailable_reconciliation_policy(&store, &url, &config)
        .await
        .expect("unavailable transaction reconciliation policy");
    forged_receipt_cannot_bind_a_direct_commit(&store, &context, &config)
        .await
        .expect("forged transaction receipt provenance");
    reused_batch_id_and_filler_guard_are_rejected(&store, &context, &config)
        .await
        .expect("reused batch identity and filler guard corruption");
    schema_four_event_with_filler_is_not_loadable(&store, &context, &config)
        .await
        .expect("schema-4 history receipt validation");

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

async fn legacy_event_store_policy_is_upgraded(
    context: &async_nats::jetstream::Context,
) -> TestResult<()> {
    let (bounded_context, stream_name) = unique_names("legacy-policy-upgrade")?;
    let config = NatsEventStoreConfig::new(&bounded_context, stream_name)?
        .with_storage_limits(64 * 1024 * 1024, 512 * 1024)?;
    let mut legacy = config.stream_config();
    legacy.subjects = vec![config.aggregate_subject_filter()];
    legacy.max_message_size = i32::try_from(config.max_event_bytes())?;
    context.create_stream(legacy).await?;

    provision_event_store(context, &config).await?;
    let upgraded = context.get_stream(config.stream_name()).await?;
    check(
        upgraded.cached_info().config.subjects == config.stream_config().subjects,
        "legacy stream subjects were not upgraded",
    )?;
    check(
        upgraded.cached_info().config.max_message_size == config.stream_config().max_message_size,
        "legacy stream message size was not upgraded",
    )
}

async fn legacy_history_remains_readable_after_lower_limit_provisioning(
    context: &async_nats::jetstream::Context,
) -> TestResult<()> {
    const HISTORICAL_WRITE_LIMIT: usize = 768 * 1024;
    const HISTORICAL_PAYLOAD_BYTES: usize = 400 * 1024;

    if context.client().max_payload() < HISTORICAL_WRITE_LIMIT + 4 * 1024 {
        eprintln!("NATS max_payload is too small for legacy event-store migration coverage");
        return Ok(());
    }
    let (bounded_context, stream_name) = unique_names("legacy-limit-migration")?;
    let current = NatsEventStoreConfig::new(&bounded_context, &stream_name)?
        .with_storage_limits(64 * 1024 * 1024, DEFAULT_EVENT_STORE_MAX_EVENT_BYTES)?;
    let historical = NatsEventStoreConfig::new(&bounded_context, &stream_name)?
        .with_storage_limits(64 * 1024 * 1024, HISTORICAL_WRITE_LIMIT)?;
    let mut legacy_policy = historical.stream_config();
    legacy_policy.max_message_size = i32::try_from(MAX_SUPPORTED_EVENT_BYTES)?;
    context.create_stream(legacy_policy).await?;

    let historical_store = NatsEventStore::connect(context.clone(), historical).await?;
    let aggregate = stream("legacy-limit-migration")?;
    let historical_payload = vec![7; HISTORICAL_PAYLOAD_BYTES];
    historical_store
        .append(
            &aggregate,
            ExpectedVersion::NoStream,
            owned_payload_batch(
                &aggregate,
                "legacy-limit-operation",
                "legacy-limit-content",
                historical_payload.clone(),
            )?,
        )
        .await?;
    let aggregate_subject = current.aggregate_subject(
        aggregate.aggregate_type().as_str(),
        aggregate.aggregate_id().as_str(),
    );
    let mut legacy_stream = context.get_stream(current.stream_name()).await?;
    let raw = legacy_stream
        .get_last_raw_message_by_subject(&aggregate_subject)
        .await?;
    check(
        raw.payload.len() > current.max_event_bytes(),
        "historical event did not exceed the current write limit",
    )?;
    check(
        raw.payload.len() <= MAX_SUPPORTED_EVENT_BYTES,
        "historical event exceeded the supported read limit",
    )?;

    let mut legacy_policy = legacy_stream.cached_info().config.clone();
    legacy_policy.subjects = vec![current.aggregate_subject_filter()];
    context.create_or_update_stream(legacy_policy).await?;
    provision_event_store(context, &current).await?;

    legacy_stream = context.get_stream(current.stream_name()).await?;
    check(
        legacy_stream.cached_info().config.max_message_size
            == i32::try_from(MAX_SUPPORTED_EVENT_BYTES)?,
        "legacy stream message capacity was not preserved",
    )?;
    check(
        legacy_stream.cached_info().config.subjects == current.stream_config().subjects,
        "legacy stream subjects were not migrated",
    )?;
    let current_store = NatsEventStore::connect(context.clone(), current.clone()).await?;
    let loaded = current_store.load(&aggregate).await?;
    check(loaded.len() == 1, "legacy history has the wrong length")?;
    check(
        loaded
            .first()
            .is_some_and(|event| event.payload() == historical_payload),
        "legacy event payload changed during migration",
    )?;

    let oversized = current_store
        .append(
            &aggregate,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            owned_payload_batch(
                &aggregate,
                "current-limit-operation",
                "current-limit-content",
                vec![8; HISTORICAL_PAYLOAD_BYTES],
            )?,
        )
        .await;
    check(
        matches!(oversized, Err(ref error) if error.kind() == EventStoreErrorKind::InvalidRequest),
        "current writes did not obey the lower configured event limit",
    )
}

async fn transaction_history_remains_readable_after_lower_limit_provisioning(
    context: &async_nats::jetstream::Context,
) -> TestResult<()> {
    const HISTORICAL_WRITE_LIMIT: usize = 64 * 1024;
    const CURRENT_WRITE_LIMIT: usize = 4 * 1024;
    const READ_ONLY_PARTICIPANTS: usize = 128;

    if context.client().max_payload() < HISTORICAL_WRITE_LIMIT + 4 * 1024 {
        eprintln!("NATS max_payload is too small for transaction migration coverage");
        return Ok(());
    }
    let (bounded_context, stream_name) = unique_names("transaction-limit-migration")?;
    let historical = NatsEventStoreConfig::new(&bounded_context, &stream_name)?
        .with_storage_limits(64 * 1024 * 1024, HISTORICAL_WRITE_LIMIT)?;
    let current = NatsEventStoreConfig::new(&bounded_context, &stream_name)?
        .with_storage_limits(64 * 1024 * 1024, CURRENT_WRITE_LIMIT)?;
    provision_event_store(context, &historical).await?;
    let historical_store = NatsEventStore::connect(context.clone(), historical).await?;
    let primary = stream("transaction-limit-migration-primary")?;
    let operation = "transaction-limit-migration-operation";
    let operation_id = OperationId::new(operation)?;
    let fingerprint = ContentFingerprint::digest(operation);
    let mut participants = Vec::with_capacity(READ_ONLY_PARTICIPANTS + 1);
    participants.push(TransactionParticipant::new(
        primary.clone(),
        ExpectedVersion::NoStream,
        Some(owned_payload_batch(
            &primary,
            operation,
            operation,
            vec![9; 8 * 1024],
        )?),
    ));
    for ordinal in 0..READ_ONLY_PARTICIPANTS {
        participants.push(TransactionParticipant::new(
            stream(&format!("transaction-limit-observed-{ordinal}"))?,
            ExpectedVersion::NoStream,
            None,
        ));
    }
    historical_store
        .append_transaction(EventTransaction::new(
            operation_id.clone(),
            fingerprint,
            participants,
        ))
        .await?;

    let mut stream_info = context.get_stream(current.stream_name()).await?;
    let event = stream_info
        .get_last_raw_message_by_subject(&current.aggregate_subject(
            primary.aggregate_type().as_str(),
            primary.aggregate_id().as_str(),
        ))
        .await?;
    let receipt = stream_info
        .get_last_raw_message_by_subject(&current.transaction_subject(&primary, operation))
        .await?;
    check(
        event.payload.len() > current.max_event_bytes(),
        "historical schema-4 event did not exceed the current write limit",
    )?;
    check(
        receipt.payload.len() > current.max_event_bytes(),
        "historical receipt did not exceed the current write limit",
    )?;
    check(
        event.payload.len() <= MAX_SUPPORTED_EVENT_BYTES
            && receipt.payload.len() <= MAX_SUPPORTED_EVENT_BYTES,
        "historical transaction data exceeded the supported read limit",
    )?;

    provision_event_store(context, &current).await?;
    stream_info = context.get_stream(current.stream_name()).await?;
    check(
        stream_info.cached_info().config.max_message_size
            == i32::try_from(HISTORICAL_WRITE_LIMIT + 4 * 1024)?,
        "historical transaction message capacity was not preserved",
    )?;
    let current_store = NatsEventStore::connect(context.clone(), current).await?;
    let loaded = current_store
        .load_transaction_receipt(&primary, &operation_id)
        .await?
        .ok_or_else(|| "historical transaction receipt was not found".to_owned())?;
    check(
        loaded.streams().len() == READ_ONLY_PARTICIPANTS + 1,
        "historical transaction receipt has the wrong participant count",
    )?;
    check(
        loaded.events().len() == 1 && current_store.load(&primary).await?.len() == 1,
        "historical schema-4 event was not materialized",
    )
}

async fn max_payload_is_validated_before_provisioning(
    context: &async_nats::jetstream::Context,
) -> TestResult<()> {
    const ATOMIC_HEADER_ALLOWANCE: usize = 4 * 1024;

    let negotiated = context.client().max_payload();
    let max_event_bytes = negotiated.min(MAX_SUPPORTED_EVENT_BYTES);
    let max_wire_message_bytes = checked_add_usize(
        max_event_bytes,
        ATOMIC_HEADER_ALLOWANCE,
        "maximum wire message bytes",
    )?;
    if max_wire_message_bytes <= negotiated {
        return Ok(());
    }
    let (bounded_context, stream_name) = unique_names("max-payload-validation")?;
    let config = NatsEventStoreConfig::new(&bounded_context, stream_name)?
        .with_storage_limits(i64::try_from(max_wire_message_bytes)?, max_event_bytes)?;

    let result = provision_event_store(context, &config).await;
    check(
        matches!(
            result,
            Err(ref error)
                if error.kind() == EventStoreErrorKind::ConfigurationMismatch
                    && error.message().contains("NATS max_payload")
        ),
        "wire messages larger than max_payload were not rejected as a configuration mismatch",
    )
}

#[allow(clippy::too_many_lines)]
async fn transaction_contract_and_wire_policy(
    store: &NatsEventStore,
    context: &async_nats::jetstream::Context,
    config: &NatsEventStoreConfig,
) -> TestResult<()> {
    let primary = stream("transaction-primary")?;
    let secondary = stream("transaction-secondary")?;
    let observed = stream("transaction-observed")?;
    let operation = "multi-stream-operation";
    let operation_id = OperationId::new(operation)?;
    let fingerprint = ContentFingerprint::digest(operation);
    let primary_batch = batch(&primary, operation, operation, &[b"debited", b"audited"])?;

    store
        .append(
            &observed,
            ExpectedVersion::NoStream,
            batch(&observed, operation, operation, &[b"observed"])?,
        )
        .await?;

    let transaction = EventTransaction::new(
        operation_id.clone(),
        fingerprint,
        vec![
            TransactionParticipant::new(
                primary.clone(),
                ExpectedVersion::NoStream,
                Some(primary_batch.clone()),
            ),
            TransactionParticipant::new(
                secondary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&secondary, operation, operation, &[b"credited"])?),
            ),
            TransactionParticipant::new(
                observed.clone(),
                ExpectedVersion::Exact(StreamVersion::new(1)),
                None,
            ),
        ],
    );
    let outcome = store.append_transaction(transaction.clone()).await?;
    check(
        matches!(&outcome, TransactionAppendOutcome::Appended(_)),
        "multi-stream transaction was not reported as appended",
    )?;
    check(
        outcome.receipt().events().len() == 3,
        "transaction receipt has the wrong event count",
    )?;
    check(
        outcome.receipt().streams().len() == 3,
        "transaction receipt has the wrong participant count",
    )?;
    let observed_receipt = outcome
        .receipt()
        .streams()
        .get(2)
        .ok_or_else(|| "transaction receipt has no observed participant".to_owned())?;
    check(
        observed_receipt.events().is_empty(),
        "read-only participant unexpectedly recorded events",
    )?;
    check(
        observed_receipt.base_version() == StreamVersion::new(1),
        "read-only participant has the wrong base version",
    )?;
    check(
        store.load(&primary).await?.len() == 2,
        "primary transaction history has the wrong length",
    )?;
    check(
        store.load(&secondary).await?.len() == 1,
        "secondary transaction history has the wrong length",
    )?;
    let direct_primary_retry = store
        .append(&primary, ExpectedVersion::NoStream, primary_batch)
        .await;
    check(
        matches!(
            direct_primary_retry,
            Err(ref error) if error.kind() == EventStoreErrorKind::IdentityConflict
        ),
        "a transaction primary batch was interpreted as a standalone append replay",
    )?;

    let mut stream_info = context.get_stream(config.stream_name()).await?;
    let primary_subject = config.aggregate_subject(
        primary.aggregate_type().as_str(),
        primary.aggregate_id().as_str(),
    );
    let secondary_subject = config.aggregate_subject(
        secondary.aggregate_type().as_str(),
        secondary.aggregate_id().as_str(),
    );
    let mut batch_id = None;
    for (subject, expected_transaction_ordinals) in [
        (primary_subject.as_str(), &[0_u64, 1][..]),
        (secondary_subject.as_str(), &[2_u64][..]),
    ] {
        let mut next_sequence = 1_u64;
        for expected_transaction_ordinal in expected_transaction_ordinals {
            let message = stream_info
                .get_first_raw_message_by_subject(subject, next_sequence)
                .await?;
            next_sequence = checked_add_u64(
                message.sequence,
                1,
                "next transaction event stream sequence",
            )?;
            let wire: serde_json::Value = serde_json::from_slice(&message.payload)?;
            check(
                wire.pointer("/schemaVersion")
                    .and_then(serde_json::Value::as_u64)
                    == Some(4),
                "transaction event has the wrong schema version",
            )?;
            check(
                wire.pointer("/event/transactionEventOrdinal")
                    .and_then(serde_json::Value::as_u64)
                    == Some(*expected_transaction_ordinal),
                "transaction event has the wrong transaction ordinal",
            )?;
            check(
                wire.pointer("/event/transactionEventCount")
                    .and_then(serde_json::Value::as_u64)
                    == Some(3),
                "transaction event has the wrong transaction event count",
            )?;
            check(
                message.headers.get("Nats-Batch-Commit").is_none(),
                "transaction event unexpectedly has a batch commit header",
            )?;
            let expected_batch_sequence = checked_add_u64(
                *expected_transaction_ordinal,
                1,
                "expected transaction batch sequence",
            )?
            .to_string();
            check(
                message
                    .headers
                    .get("Nats-Batch-Sequence")
                    .map(async_nats::HeaderValue::as_str)
                    == Some(expected_batch_sequence.as_str()),
                "transaction event has the wrong batch sequence",
            )?;
            let stored_batch_id = message
                .headers
                .get("Nats-Batch-Id")
                .ok_or_else(|| "transaction event has no batch identity".to_owned())?
                .as_str();
            if let Some(expected_batch_id) = &batch_id {
                check(
                    stored_batch_id == expected_batch_id,
                    "transaction events do not share one batch identity",
                )?;
            } else {
                batch_id = Some(stored_batch_id.to_owned());
            }
        }
    }

    let observed_subject = config.aggregate_subject(
        observed.aggregate_type().as_str(),
        observed.aggregate_id().as_str(),
    );
    let observed_message = stream_info
        .get_last_raw_message_by_subject(&observed_subject)
        .await?;
    let guard_subject = config.transaction_guard_subject(&primary, operation, 0);
    let guard = stream_info
        .get_last_raw_message_by_subject(&guard_subject)
        .await?;
    check(
        guard
            .headers
            .get("Nats-Expected-Last-Subject-Sequence-Subject")
            .map(async_nats::HeaderValue::as_str)
            == Some(observed_subject.as_str()),
        "read guard targets the wrong subject",
    )?;
    let observed_sequence = observed_message.sequence.to_string();
    check(
        guard
            .headers
            .get("Nats-Expected-Last-Subject-Sequence")
            .map(async_nats::HeaderValue::as_str)
            == Some(observed_sequence.as_str()),
        "read guard has the wrong expected sequence",
    )?;
    check(
        guard
            .headers
            .get("Nats-Batch-Sequence")
            .map(async_nats::HeaderValue::as_str)
            == Some("4"),
        "read guard has the wrong batch sequence",
    )?;
    let guard_wire: serde_json::Value = serde_json::from_slice(&guard.payload)?;
    check(
        guard_wire
            .pointer("/operationId")
            .and_then(serde_json::Value::as_str)
            == Some(operation),
        "read guard has the wrong operation identity",
    )?;

    let receipt_subject = config.transaction_subject(&primary, operation);
    let receipt = stream_info
        .get_last_raw_message_by_subject(&receipt_subject)
        .await?;
    check(
        receipt
            .headers
            .get("Nats-Batch-Sequence")
            .map(async_nats::HeaderValue::as_str)
            == Some("5"),
        "transaction receipt has the wrong batch sequence",
    )?;
    check(
        receipt
            .headers
            .get("Nats-Batch-Commit")
            .map(async_nats::HeaderValue::as_str)
            == Some("1"),
        "transaction receipt has no batch commit marker",
    )?;
    check(
        receipt
            .headers
            .get("Nats-Expected-Last-Subject-Sequence")
            .map(async_nats::HeaderValue::as_str)
            == Some("0"),
        "transaction receipt has the wrong duplicate guard",
    )?;
    check(
        receipt
            .headers
            .get("Nats-Batch-Id")
            .map(async_nats::HeaderValue::as_str)
            == batch_id.as_deref(),
        "transaction receipt has the wrong batch identity",
    )?;
    let receipt_wire: serde_json::Value = serde_json::from_slice(&receipt.payload)?;
    check(
        receipt_wire
            .pointer("/schemaVersion")
            .and_then(serde_json::Value::as_u64)
            == Some(1),
        "transaction receipt has the wrong schema version",
    )?;
    check(
        receipt_wire
            .pointer("/receipt/operationId")
            .and_then(serde_json::Value::as_str)
            == Some(operation),
        "transaction receipt has the wrong operation identity",
    )?;
    check(
        receipt_wire
            .pointer("/receipt/participants")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            == Some(3),
        "transaction receipt has the wrong participant count",
    )?;

    let messages_before_replay = stream_info.info().await?.state.messages;
    let replay = store.append_transaction(transaction.clone()).await?;
    check(
        replay.is_exact_replay(),
        "exact transaction retry was not reported as a replay",
    )?;
    check(
        replay.receipt().events() == outcome.receipt().events(),
        "exact transaction replay returned a different receipt",
    )?;
    let changed_expectations = EventTransaction::new(
        operation_id.clone(),
        fingerprint,
        vec![
            TransactionParticipant::new(
                primary.clone(),
                ExpectedVersion::Exact(StreamVersion::ZERO),
                Some(batch(
                    &primary,
                    operation,
                    operation,
                    &[b"debited", b"audited"],
                )?),
            ),
            TransactionParticipant::new(
                secondary.clone(),
                ExpectedVersion::Exact(StreamVersion::new(99)),
                Some(batch(&secondary, operation, operation, &[b"credited"])?),
            ),
            TransactionParticipant::new(observed.clone(), ExpectedVersion::NoStream, None),
        ],
    );
    check(
        store
            .append_transaction(changed_expectations)
            .await?
            .is_exact_replay(),
        "exact retry did not ignore changed expectations",
    )?;
    check(
        stream_info.info().await?.state.messages == messages_before_replay,
        "exact replay published another transaction",
    )?;
    check(
        store
            .load_transaction_receipt(&primary, &operation_id)
            .await?
            .is_some(),
        "transaction receipt lookup returned no receipt",
    )?;

    let changed = EventTransaction::new(
        operation_id,
        ContentFingerprint::digest("changed-transaction"),
        vec![
            TransactionParticipant::new(
                primary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(
                    &primary,
                    operation,
                    "changed-transaction",
                    &[b"changed"],
                )?),
            ),
            TransactionParticipant::new(
                observed,
                ExpectedVersion::Exact(StreamVersion::new(1)),
                None,
            ),
        ],
    );
    let changed_result = store.append_transaction(changed).await;
    check(
        matches!(
            changed_result,
            Err(ref error) if error.kind() == EventStoreErrorKind::IdentityConflict
        ),
        "reused primary transaction identity did not conflict",
    )?;

    let stale_stream = stream("transaction-stale-read")?;
    store
        .append(
            &stale_stream,
            ExpectedVersion::NoStream,
            batch(
                &stale_stream,
                "transaction-stale-seed",
                "transaction-stale-seed",
                &[b"seed"],
            )?,
        )
        .await?;
    let untouched = stream("transaction-conflict-untouched")?;
    let conflict_operation = "transaction-conflict";
    let conflict = store
        .append_transaction(EventTransaction::new(
            OperationId::new(conflict_operation)?,
            ContentFingerprint::digest(conflict_operation),
            vec![
                TransactionParticipant::new(
                    untouched.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch(
                        &untouched,
                        conflict_operation,
                        conflict_operation,
                        &[b"must-not-append"],
                    )?),
                ),
                TransactionParticipant::new(stale_stream, ExpectedVersion::NoStream, None),
            ],
        ))
        .await;
    check(
        matches!(conflict, Err(ref error) if error.kind() == EventStoreErrorKind::Conflict),
        "stale read guard did not reject the transaction",
    )?;
    check(
        store.load(&untouched).await?.is_empty(),
        "a rejected transaction appended an event prefix",
    )?;

    let oversized_primary = stream("transaction-oversized-before-io")?;
    let oversized_operation = "transaction-oversized-before-io";
    context
        .send_publish(
            config.transaction_subject(&oversized_primary, oversized_operation),
            PublishMessage::build()
                .payload(br#"{"corrupt":"receipt"}"#.to_vec().into())
                .expected_stream(config.stream_name()),
        )
        .await?
        .await?;
    let oversized = store
        .append_transaction(EventTransaction::new(
            OperationId::new(oversized_operation)?,
            ContentFingerprint::digest(oversized_operation),
            vec![TransactionParticipant::new(
                oversized_primary.clone(),
                ExpectedVersion::NoStream,
                Some(repeated_batch(
                    &oversized_primary,
                    oversized_operation,
                    oversized_operation,
                    u32::try_from(MAX_TRANSACTION_ITEMS)?,
                )?),
            )],
        ))
        .await;
    check(
        matches!(
            oversized,
            Err(ref error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
                    && error.message().contains("item limit")
        ),
        "an oversized transaction was not rejected before receipt I/O",
    )?;
    check(
        store.load(&oversized_primary).await?.is_empty(),
        "an early oversized rejection appended domain events",
    )
}

async fn concurrent_transaction_identity_race(store: &NatsEventStore) -> TestResult<()> {
    let shared = stream("transaction-identity-race-shared")?;
    let first_primary = stream("transaction-identity-race-primary-a")?;
    let second_primary = stream("transaction-identity-race-primary-b")?;
    let operation = "transaction-identity-race";
    let operation_id = OperationId::new(operation)?;
    let fingerprint = ContentFingerprint::digest(operation);
    let shared_batch = batch(&shared, operation, operation, &[b"shared"])?;
    let first = EventTransaction::new(
        operation_id.clone(),
        fingerprint,
        vec![
            TransactionParticipant::new(
                first_primary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&first_primary, operation, operation, &[b"first"])?),
            ),
            TransactionParticipant::new(
                shared.clone(),
                ExpectedVersion::NoStream,
                Some(shared_batch.clone()),
            ),
        ],
    );
    let second = EventTransaction::new(
        operation_id,
        fingerprint,
        vec![
            TransactionParticipant::new(
                second_primary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&second_primary, operation, operation, &[b"second"])?),
            ),
            TransactionParticipant::new(
                shared.clone(),
                ExpectedVersion::NoStream,
                Some(shared_batch),
            ),
        ],
    );

    let results: [_; 2] = tokio::join!(
        store.append_transaction(first),
        store.append_transaction(second),
    )
    .into();
    check(
        results.iter().filter(|result| result.is_ok()).count() == 1,
        "concurrent transactions with a shared identity did not have one winner",
    )?;
    check(
        results
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(error) if error.kind() == EventStoreErrorKind::IdentityConflict
                )
            })
            .count()
            == 1,
        "the losing transaction did not report an identity conflict",
    )?;
    check(
        store.load(&shared).await?.len() == 1,
        "the identity race did not append exactly one shared event",
    )
}

async fn concurrent_identical_transaction_reloads_receipt(
    store: &NatsEventStore,
) -> TestResult<()> {
    let primary = stream("transaction-identical-race-primary")?;
    let secondary = stream("transaction-identical-race-secondary")?;
    let operation = "transaction-identical-race";
    let transaction = EventTransaction::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(operation),
        vec![
            TransactionParticipant::new(
                primary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&primary, operation, operation, &[b"primary"])?),
            ),
            TransactionParticipant::new(
                secondary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&secondary, operation, operation, &[b"secondary"])?),
            ),
        ],
    );
    let receipt_missed = Arc::new(Barrier::new(2));
    let release_retry = Arc::new(Barrier::new(2));
    let delayed_store = store.clone().with_transaction_receipt_miss_barriers(
        Arc::clone(&receipt_missed),
        Arc::clone(&release_retry),
    );

    let (delayed, winner) = tokio::join!(
        delayed_store.append_transaction(transaction.clone()),
        async {
            let _ = receipt_missed.wait().await;
            let result = store.append_transaction(transaction).await;
            let _ = release_retry.wait().await;
            result
        },
    );

    check(
        matches!(winner, Ok(TransactionAppendOutcome::Appended(_))),
        "the synchronized transaction did not commit while its retry was paused",
    )?;
    check(
        matches!(delayed, Ok(TransactionAppendOutcome::ExactReplay(_))),
        "the delayed identical transaction did not reload its committed receipt",
    )?;
    check(
        store.load(&primary).await?.len() == 1 && store.load(&secondary).await?.len() == 1,
        "the concurrent identical transaction wrote more than one batch",
    )
}

#[allow(clippy::too_many_lines)]
async fn unavailable_reconciliation_policy(
    store: &NatsEventStore,
    url: &str,
    config: &NatsEventStoreConfig,
) -> TestResult<()> {
    let read_only_operation = "unavailable-reconcile-read-only";
    let read_only_primary = stream("unavailable-reconcile-read-only-primary")?;
    let observed = stream("unavailable-reconcile-observed")?;
    store
        .append(
            &observed,
            ExpectedVersion::NoStream,
            batch(
                &observed,
                read_only_operation,
                read_only_operation,
                &[b"existing observation"],
            )?,
        )
        .await?;
    let read_only_transaction = EventTransaction::new(
        OperationId::new(read_only_operation)?,
        ContentFingerprint::digest(read_only_operation),
        vec![
            TransactionParticipant::new(
                read_only_primary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(
                    &read_only_primary,
                    read_only_operation,
                    read_only_operation,
                    &[b"write"],
                )?),
            ),
            TransactionParticipant::new(
                observed,
                ExpectedVersion::Exact(StreamVersion::new(1)),
                None,
            ),
        ],
    );
    let unknown_publish = EventStoreError::new(
        EventStoreErrorKind::Unavailable,
        "atomic publish outcome is unknown",
    );
    let read_only_result = store
        .reconcile_unavailable_transaction_publish(&read_only_transaction, unknown_publish.clone())
        .await;
    check(
        matches!(&read_only_result, Err(error) if error == &unknown_publish),
        "a read-only participant identity replaced the original unavailable error",
    )?;

    let writer_operation = "unavailable-reconcile-exact-writer";
    let writer_primary = stream("unavailable-reconcile-writer-primary")?;
    let existing_writer = stream("unavailable-reconcile-existing-writer")?;
    let existing_batch = batch(
        &existing_writer,
        writer_operation,
        writer_operation,
        &[b"existing write"],
    )?;
    store
        .append(
            &existing_writer,
            ExpectedVersion::NoStream,
            existing_batch.clone(),
        )
        .await?;
    let writer_transaction = EventTransaction::new(
        OperationId::new(writer_operation)?,
        ContentFingerprint::digest(writer_operation),
        vec![
            TransactionParticipant::new(
                writer_primary.clone(),
                ExpectedVersion::NoStream,
                Some(batch(
                    &writer_primary,
                    writer_operation,
                    writer_operation,
                    &[b"new write"],
                )?),
            ),
            TransactionParticipant::new(
                existing_writer,
                ExpectedVersion::NoStream,
                Some(existing_batch),
            ),
        ],
    );
    let writer_result = store
        .reconcile_unavailable_transaction_publish(&writer_transaction, unknown_publish.clone())
        .await;
    check(
        matches!(
            writer_result,
            Err(ref error) if error.kind() == EventStoreErrorKind::IdentityConflict
        ),
        "an exact writing participant identity was not detected during reconciliation",
    )?;

    let transient_context = connect_context(url).await?;
    let transient_client = transient_context.client();
    let transient_store = NatsEventStore::connect(transient_context, config.clone()).await?;
    transient_client.drain().await?;
    tokio::task::yield_now().await;
    let transient_operation = "unavailable-reconcile-transient-receipt";
    let transient_primary = stream("unavailable-reconcile-transient-primary")?;
    let transient_transaction = EventTransaction::new(
        OperationId::new(transient_operation)?,
        ContentFingerprint::digest(transient_operation),
        vec![TransactionParticipant::new(
            transient_primary.clone(),
            ExpectedVersion::NoStream,
            Some(batch(
                &transient_primary,
                transient_operation,
                transient_operation,
                &[b"unknown"],
            )?),
        )],
    );
    let transient_result = transient_store
        .reconcile_unavailable_transaction_publish(&transient_transaction, unknown_publish.clone())
        .await;
    check(
        matches!(&transient_result, Err(error) if error == &unknown_publish),
        "a transient receipt lookup did not preserve the original unavailable error",
    )
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedReceiptContent<'a> {
    event_store_stream: &'a str,
    application: &'a str,
    bounded_context: &'a str,
    operation_id: &'a str,
    operation_fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    causation_id: Option<&'a str>,
    participants: Vec<ForgedReceiptParticipant<'a>>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedReceiptParticipant<'a> {
    stream: ForgedStreamIdentity<'a>,
    base_stream_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_id: Option<&'a str>,
    event_count: u32,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedStreamIdentity<'a> {
    aggregate_type: &'a str,
    aggregate_id: &'a str,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedReceiptChecksumInput<'a> {
    schema_version: u16,
    receipt: &'a ForgedReceiptContent<'a>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedReceiptWire<'a> {
    schema_version: u16,
    checksum: String,
    receipt: &'a ForgedReceiptContent<'a>,
}

async fn forged_receipt_cannot_bind_a_direct_commit(
    store: &NatsEventStore,
    context: &async_nats::jetstream::Context,
    config: &NatsEventStoreConfig,
) -> TestResult<()> {
    let primary = stream("forged-receipt-direct-commit")?;
    let operation = "forged-receipt-direct-operation";
    let direct_batch = batch(&primary, operation, operation, &[b"direct"])?;
    store
        .append(&primary, ExpectedVersion::NoStream, direct_batch.clone())
        .await?;

    let receipt = ForgedReceiptContent {
        event_store_stream: config.stream_name(),
        application: config.application().as_str(),
        bounded_context: config.bounded_context().as_str(),
        operation_id: direct_batch.operation_id().as_str(),
        operation_fingerprint: direct_batch.operation_fingerprint().to_hex(),
        correlation_id: None,
        causation_id: None,
        participants: vec![ForgedReceiptParticipant {
            stream: ForgedStreamIdentity {
                aggregate_type: primary.aggregate_type().as_str(),
                aggregate_id: primary.aggregate_id().as_str(),
            },
            base_stream_version: 0,
            commit_id: Some(direct_batch.commit_id().as_str()),
            event_count: 1,
        }],
    };
    let checksum_input = serde_json::to_vec(&ForgedReceiptChecksumInput {
        schema_version: 1,
        receipt: &receipt,
    })?;
    let payload = serde_json::to_vec(&ForgedReceiptWire {
        schema_version: 1,
        checksum: hex::encode_lower_hex(Sha256::digest(checksum_input)),
        receipt: &receipt,
    })?;
    let batch_id = "forged-receipt-atomic-batch";

    let mut guard_headers = async_nats::HeaderMap::new();
    guard_headers.insert("Content-Type", "application/json");
    guard_headers.insert("Nats-Required-Api-Level", "2");
    guard_headers.insert("Nats-Batch-Id", batch_id);
    guard_headers.insert("Nats-Batch-Sequence", "1");
    guard_headers.insert("Nats-Expected-Stream", config.stream_name());
    let guard_response = context
        .client()
        .send_request(
            config.transaction_guard_subject(&primary, operation, 0),
            Request::new()
                .headers(guard_headers)
                .payload(br"{}".to_vec().into())
                .timeout(Some(config.puback_timeout())),
        )
        .await?;
    check(
        guard_response.payload.is_empty(),
        "NATS unexpectedly acknowledged a forged batch before its receipt",
    )?;

    let mut receipt_headers = async_nats::HeaderMap::new();
    receipt_headers.insert("Content-Type", "application/json");
    receipt_headers.insert("Nats-Required-Api-Level", "2");
    receipt_headers.insert("Nats-Batch-Id", batch_id);
    receipt_headers.insert("Nats-Batch-Sequence", "2");
    receipt_headers.insert("Nats-Batch-Commit", "1");
    receipt_headers.insert("Nats-Expected-Last-Subject-Sequence", "0");
    let receipt_response = context
        .client()
        .send_request(
            config.transaction_subject(&primary, operation),
            Request::new()
                .headers(receipt_headers)
                .payload(payload.into())
                .timeout(Some(config.puback_timeout())),
        )
        .await?;
    check(
        !receipt_response.payload.is_empty(),
        "NATS omitted the forged receipt batch PubAck",
    )?;

    let loaded = store
        .load_transaction_receipt(&primary, direct_batch.operation_id())
        .await;
    check(
        matches!(loaded, Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory),
        "a forged receipt bound a separately appended direct commit",
    )?;
    let replay = store
        .append(&primary, ExpectedVersion::NoStream, direct_batch)
        .await;
    check(
        matches!(replay, Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory),
        "a malformed receipt did not make direct append history corrupt",
    )
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedStoredEventContent<'a> {
    event_store_stream: &'a str,
    application: &'a str,
    bounded_context: &'a str,
    stream: ForgedStreamIdentity<'a>,
    stream_version: u64,
    commit_id: &'a str,
    operation_id: &'a str,
    operation_fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    causation_id: Option<&'a str>,
    commit_event_ordinal: u32,
    commit_event_count: u32,
    transaction_event_ordinal: u32,
    transaction_event_count: u32,
    event_id: &'a str,
    event_type: &'a str,
    event_schema_version: u32,
    payload_base64: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedEventChecksumInput<'a> {
    schema_version: u16,
    event: &'a ForgedStoredEventContent<'a>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedStoredEventWire<'a> {
    schema_version: u16,
    checksum: String,
    event: &'a ForgedStoredEventContent<'a>,
}

struct RawAtomicMessage {
    subject: String,
    payload: Vec<u8>,
    expected_last_subject_sequence: Option<u64>,
    expectation_subject: Option<String>,
}

fn forged_transaction_event_payload<'a>(
    config: &'a NatsEventStoreConfig,
    stream_id: &'a StreamId,
    batch: &'a EventBatch,
) -> TestResult<Vec<u8>> {
    let event = batch
        .events()
        .first()
        .ok_or_else(|| "forged transaction batch has no event".to_owned())?;
    let content = ForgedStoredEventContent {
        event_store_stream: config.stream_name(),
        application: config.application().as_str(),
        bounded_context: config.bounded_context().as_str(),
        stream: ForgedStreamIdentity {
            aggregate_type: stream_id.aggregate_type().as_str(),
            aggregate_id: stream_id.aggregate_id().as_str(),
        },
        stream_version: 1,
        commit_id: batch.commit_id().as_str(),
        operation_id: batch.operation_id().as_str(),
        operation_fingerprint: batch.operation_fingerprint().to_hex(),
        correlation_id: batch
            .correlation_id()
            .map(rostfrei_messaging_core::CorrelationId::as_str),
        causation_id: batch
            .causation_id()
            .map(rostfrei_messaging_core::CausationId::as_str),
        commit_event_ordinal: 0,
        commit_event_count: 1,
        transaction_event_ordinal: 0,
        transaction_event_count: 1,
        event_id: event.event_id().as_str(),
        event_type: event.event_type(),
        event_schema_version: event.schema_version(),
        payload_base64: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            event.payload(),
        ),
    };
    let checksum = hex::encode_lower_hex(Sha256::digest(serde_json::to_vec(
        &ForgedEventChecksumInput {
            schema_version: 4,
            event: &content,
        },
    )?));
    Ok(serde_json::to_vec(&ForgedStoredEventWire {
        schema_version: 4,
        checksum,
        event: &content,
    })?)
}

fn forged_receipt_payload(receipt: &ForgedReceiptContent<'_>) -> TestResult<Vec<u8>> {
    let checksum = hex::encode_lower_hex(Sha256::digest(serde_json::to_vec(
        &ForgedReceiptChecksumInput {
            schema_version: 1,
            receipt,
        },
    )?));
    Ok(serde_json::to_vec(&ForgedReceiptWire {
        schema_version: 1,
        checksum,
        receipt,
    })?)
}

async fn publish_raw_atomic_batch(
    context: &async_nats::jetstream::Context,
    config: &NatsEventStoreConfig,
    batch_id: &str,
    messages: Vec<RawAtomicMessage>,
) -> TestResult<()> {
    let message_count = messages.len();
    for (index, message) in messages.into_iter().enumerate() {
        let sequence = checked_add_usize(index, 1, "raw atomic batch sequence")?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert("Nats-Required-Api-Level", "2");
        headers.insert("Nats-Batch-Id", batch_id);
        headers.insert("Nats-Batch-Sequence", sequence.to_string());
        if index == 0 {
            headers.insert("Nats-Expected-Stream", config.stream_name());
        }
        if let Some(expected) = message.expected_last_subject_sequence {
            headers.insert("Nats-Expected-Last-Subject-Sequence", expected.to_string());
        }
        if let Some(subject) = message.expectation_subject {
            headers.insert("Nats-Expected-Last-Subject-Sequence-Subject", subject);
        }
        if sequence == message_count {
            headers.insert("Nats-Batch-Commit", "1");
        }
        let response = context
            .client()
            .send_request(
                message.subject,
                Request::new()
                    .headers(headers)
                    .payload(message.payload.into())
                    .timeout(Some(config.puback_timeout())),
            )
            .await?;
        check(
            response.payload.is_empty() == (sequence != message_count),
            "raw atomic batch returned an unexpected acknowledgement",
        )?;
        if sequence == message_count {
            let acknowledgement: serde_json::Value = serde_json::from_slice(&response.payload)?;
            check(
                acknowledgement.get("error").is_none()
                    && acknowledgement
                        .get("stream")
                        .and_then(serde_json::Value::as_str)
                        == Some(config.stream_name()),
                "raw atomic batch was not committed",
            )?;
        }
    }
    Ok(())
}

async fn schema_four_event_with_filler_is_not_loadable(
    store: &NatsEventStore,
    context: &async_nats::jetstream::Context,
    config: &NatsEventStoreConfig,
) -> TestResult<()> {
    let aggregate = stream("schema-four-event-with-filler")?;
    let operation = "schema-four-event-with-filler-operation";
    let event_batch = batch(&aggregate, operation, operation, &[b"must-not-load"])?;
    publish_raw_atomic_batch(
        context,
        config,
        "schema-four-event-with-filler-batch",
        vec![
            RawAtomicMessage {
                subject: config.aggregate_subject(
                    aggregate.aggregate_type().as_str(),
                    aggregate.aggregate_id().as_str(),
                ),
                payload: forged_transaction_event_payload(config, &aggregate, &event_batch)?,
                expected_last_subject_sequence: Some(0),
                expectation_subject: None,
            },
            RawAtomicMessage {
                subject: config.transaction_guard_subject(&aggregate, "unrelated-filler", 0),
                payload: br#"{"unrelated":true}"#.to_vec(),
                expected_last_subject_sequence: None,
                expectation_subject: None,
            },
        ],
    )
    .await?;

    let loaded = store.load(&aggregate).await;
    check(
        matches!(loaded, Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory),
        "schema-4 history was exposed without a valid transaction receipt",
    )
}

#[allow(clippy::too_many_lines)]
async fn reused_batch_id_and_filler_guard_are_rejected(
    store: &NatsEventStore,
    context: &async_nats::jetstream::Context,
    config: &NatsEventStoreConfig,
) -> TestResult<()> {
    let reused_primary = stream("reused-batch-id-primary")?;
    let reused_operation = "reused-batch-id-operation";
    let reused_batch = batch(
        &reused_primary,
        reused_operation,
        reused_operation,
        &[b"event"],
    )?;
    let reused_batch_id = "reused-atomic-batch-id";
    let reused_event_subject = config.aggregate_subject(
        reused_primary.aggregate_type().as_str(),
        reused_primary.aggregate_id().as_str(),
    );
    publish_raw_atomic_batch(
        context,
        config,
        reused_batch_id,
        vec![
            RawAtomicMessage {
                subject: reused_event_subject,
                payload: forged_transaction_event_payload(config, &reused_primary, &reused_batch)?,
                expected_last_subject_sequence: Some(0),
                expectation_subject: None,
            },
            RawAtomicMessage {
                subject: config.transaction_guard_subject(&reused_primary, reused_operation, 99),
                payload: br"{}".to_vec(),
                expected_last_subject_sequence: None,
                expectation_subject: None,
            },
        ],
    )
    .await?;
    let reused_receipt = ForgedReceiptContent {
        event_store_stream: config.stream_name(),
        application: config.application().as_str(),
        bounded_context: config.bounded_context().as_str(),
        operation_id: reused_operation,
        operation_fingerprint: reused_batch.operation_fingerprint().to_hex(),
        correlation_id: None,
        causation_id: None,
        participants: vec![ForgedReceiptParticipant {
            stream: ForgedStreamIdentity {
                aggregate_type: reused_primary.aggregate_type().as_str(),
                aggregate_id: reused_primary.aggregate_id().as_str(),
            },
            base_stream_version: 0,
            commit_id: Some(reused_batch.commit_id().as_str()),
            event_count: 1,
        }],
    };
    publish_raw_atomic_batch(
        context,
        config,
        reused_batch_id,
        vec![
            RawAtomicMessage {
                subject: config.transaction_guard_subject(&reused_primary, reused_operation, 98),
                payload: br"{}".to_vec(),
                expected_last_subject_sequence: None,
                expectation_subject: None,
            },
            RawAtomicMessage {
                subject: config.transaction_subject(&reused_primary, reused_operation),
                payload: forged_receipt_payload(&reused_receipt)?,
                expected_last_subject_sequence: Some(0),
                expectation_subject: None,
            },
        ],
    )
    .await?;
    let reused_result = store
        .load_transaction_receipt(&reused_primary, reused_batch.operation_id())
        .await;
    check(
        matches!(
            reused_result,
            Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory
        ),
        "a receipt accepted events from an earlier batch with a reused batch identity",
    )?;

    let guard_primary = stream("filler-guard-primary")?;
    let guarded = stream("filler-guard-observed")?;
    let guard_operation = "filler-guard-operation";
    let guard_batch = batch(
        &guard_primary,
        guard_operation,
        guard_operation,
        &[b"event"],
    )?;
    let guard_batch_id = "filler-guard-atomic-batch";
    let guarded_subject = config.aggregate_subject(
        guarded.aggregate_type().as_str(),
        guarded.aggregate_id().as_str(),
    );
    let guard_receipt = ForgedReceiptContent {
        event_store_stream: config.stream_name(),
        application: config.application().as_str(),
        bounded_context: config.bounded_context().as_str(),
        operation_id: guard_operation,
        operation_fingerprint: guard_batch.operation_fingerprint().to_hex(),
        correlation_id: None,
        causation_id: None,
        participants: vec![
            ForgedReceiptParticipant {
                stream: ForgedStreamIdentity {
                    aggregate_type: guard_primary.aggregate_type().as_str(),
                    aggregate_id: guard_primary.aggregate_id().as_str(),
                },
                base_stream_version: 0,
                commit_id: Some(guard_batch.commit_id().as_str()),
                event_count: 1,
            },
            ForgedReceiptParticipant {
                stream: ForgedStreamIdentity {
                    aggregate_type: guarded.aggregate_type().as_str(),
                    aggregate_id: guarded.aggregate_id().as_str(),
                },
                base_stream_version: 0,
                commit_id: None,
                event_count: 0,
            },
        ],
    };
    publish_raw_atomic_batch(
        context,
        config,
        guard_batch_id,
        vec![
            RawAtomicMessage {
                subject: config.aggregate_subject(
                    guard_primary.aggregate_type().as_str(),
                    guard_primary.aggregate_id().as_str(),
                ),
                payload: forged_transaction_event_payload(config, &guard_primary, &guard_batch)?,
                expected_last_subject_sequence: Some(0),
                expectation_subject: None,
            },
            RawAtomicMessage {
                subject: config.transaction_guard_subject(&guard_primary, guard_operation, 0),
                payload: br"{}".to_vec(),
                expected_last_subject_sequence: Some(0),
                expectation_subject: Some(guarded_subject),
            },
            RawAtomicMessage {
                subject: config.transaction_subject(&guard_primary, guard_operation),
                payload: forged_receipt_payload(&guard_receipt)?,
                expected_last_subject_sequence: Some(0),
                expectation_subject: None,
            },
        ],
    )
    .await?;
    let guard_result = store
        .load_transaction_receipt(&guard_primary, guard_batch.operation_id())
        .await;
    check(
        matches!(
            guard_result,
            Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory
        ),
        "a filler message was accepted as a transaction read guard",
    )
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
        .with_storage_limits(6 * 1024, 2048)?;
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
