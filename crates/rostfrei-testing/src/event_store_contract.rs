use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreErrorKind, ExecutionMetadata, ExpectedVersion, NewEvent, OperationId, StreamId,
    StreamVersion,
};
use rostfrei_messaging_core::{CausationId, CorrelationId};

pub async fn run<Factory, Store>(make_store: Factory)
where
    Factory: Fn() -> Store,
    Store: EventStore,
{
    empty_load(&make_store()).await;
    no_stream_and_exact_versions(&make_store()).await;
    atomic_ordered_batch(&make_store()).await;
    stream_isolation(&make_store()).await;
    identities_are_stream_scoped(&make_store()).await;
    conflict_leaves_history_unchanged(&make_store()).await;
    exact_retry(&make_store()).await;
    identity_conflicts(&make_store()).await;
    concurrent_append_has_one_winner(&make_store()).await;
}

pub async fn empty_load<Store: EventStore>(store: &Store) {
    let loaded = store
        .load(&stream("empty"))
        .await
        .expect("empty stream load should succeed");
    assert!(loaded.is_empty(), "an absent stream must load as empty");
}

pub async fn no_stream_and_exact_versions<Store: EventStore>(store: &Store) {
    let version_stream = stream("versions");
    let first = batch(&version_stream, "versions-1", "one", &[b"first"]);
    let outcome = store
        .append(&version_stream, ExpectedVersion::NoStream, first)
        .await
        .expect("NoStream should append to an absent stream");
    assert_eq!(outcome.events()[0].stream_version(), StreamVersion::new(1));

    let second = batch(&version_stream, "versions-2", "two", &[b"second"]);
    let outcome = store
        .append(
            &version_stream,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            second,
        )
        .await
        .expect("Exact should append at the matching current version");
    assert_eq!(outcome.events()[0].stream_version(), StreamVersion::new(2));

    let exact_zero_stream = stream("exact-zero");
    let invalid = batch(&exact_zero_stream, "exact-zero", "zero", &[b"event"]);
    let error = store
        .append(
            &exact_zero_stream,
            ExpectedVersion::Exact(StreamVersion::ZERO),
            invalid,
        )
        .await
        .expect_err("Exact zero must not alias NoStream");
    assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
}

pub async fn atomic_ordered_batch<Store: EventStore>(store: &Store) {
    let stream = stream("atomic");
    let outcome = store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            batch(
                &stream,
                "atomic-operation",
                "atomic-content",
                &[b"first", b"second", b"third"],
            ),
        )
        .await
        .expect("multi-event append should succeed");
    assert!(matches!(outcome, AppendOutcome::Appended(_)));

    let loaded = store.load(&stream).await.expect("load should succeed");
    assert_eq!(loaded.len(), 3);
    for (index, event) in loaded.iter().enumerate() {
        assert_eq!(event.stream_version().value(), index as u64 + 1);
        assert_eq!(
            event.commit_event_ordinal(),
            u32::try_from(index).expect("three-event commit ordinal")
        );
        assert_eq!(event.commit_event_count(), 3);
    }
    assert_eq!(loaded[0].payload(), b"first");
    assert_eq!(loaded[1].payload(), b"second");
    assert_eq!(loaded[2].payload(), b"third");
    assert!(
        loaded
            .windows(2)
            .all(|pair| pair[0].commit_id() == pair[1].commit_id())
    );
}

pub async fn stream_isolation<Store: EventStore>(store: &Store) {
    let first_stream = stream("isolated-a");
    let second_stream = stream("isolated-b");
    store
        .append(
            &first_stream,
            ExpectedVersion::NoStream,
            batch(&first_stream, "isolation-a", "a", &[b"a"]),
        )
        .await
        .expect("first stream append should succeed");
    store
        .append(
            &second_stream,
            ExpectedVersion::NoStream,
            batch(&second_stream, "isolation-b", "b", &[b"b"]),
        )
        .await
        .expect("second stream must have an independent version gate");

    let first = store
        .load(&first_stream)
        .await
        .expect("load should succeed");
    let second = store
        .load(&second_stream)
        .await
        .expect("load should succeed");
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].payload(), b"a");
    assert_eq!(second[0].payload(), b"b");
}

pub async fn identities_are_stream_scoped<Store: EventStore>(store: &Store) {
    let first_stream = stream("identity-scope-a");
    let second_stream = StreamId::new(
        AggregateType::new("OtherContractAggregate").expect("valid aggregate type"),
        AggregateId::new("identity-scope-b").expect("valid aggregate id"),
    );
    store
        .append(
            &first_stream,
            ExpectedVersion::NoStream,
            batch(&first_stream, "shared-operation", "same", &[b"first"]),
        )
        .await
        .expect("first stream append should succeed");
    store
        .append(
            &second_stream,
            ExpectedVersion::NoStream,
            batch(&second_stream, "shared-operation", "same", &[b"second"]),
        )
        .await
        .expect("operation identities are scoped to one aggregate stream");
}

pub async fn conflict_leaves_history_unchanged<Store: EventStore>(store: &Store) {
    let stream = stream("unchanged");
    store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            batch(&stream, "unchanged-1", "first", &[b"accepted"]),
        )
        .await
        .expect("initial append should succeed");
    let before = store.load(&stream).await.expect("load should succeed");

    let error = store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            batch(
                &stream,
                "unchanged-2",
                "conflicting",
                &[b"not", b"appended"],
            ),
        )
        .await
        .expect_err("stale expected version should conflict");
    assert_eq!(error.kind(), EventStoreErrorKind::Conflict);
    let after = store.load(&stream).await.expect("load should succeed");
    assert_eq!(after, before, "a failed batch must append no prefix");
}

pub async fn exact_retry<Store: EventStore>(store: &Store) {
    let stream = stream("retry");
    let retry_batch = batch(&stream, "retry-operation", "same", &[b"one", b"two"])
        .with_correlation_id(CorrelationId::new("retry-correlation").expect("correlation ID"))
        .with_causation_id(CausationId::new("retry-causation").expect("causation ID"));
    let first = store
        .append(&stream, ExpectedVersion::NoStream, retry_batch.clone())
        .await
        .expect("initial append should succeed");
    assert!(matches!(first, AppendOutcome::Appended(_)));

    let later = batch(&stream, "later-operation", "later", &[b"later"]);
    store
        .append(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(2)),
            later,
        )
        .await
        .expect("later commit should succeed");

    let replay = store
        .append(&stream, ExpectedVersion::NoStream, retry_batch)
        .await
        .expect("exact retry must succeed despite a now-stale expectation");
    assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
    assert_eq!(replay.events(), first.events());
    assert_eq!(
        replay.events()[0]
            .correlation_id()
            .expect("stored correlation")
            .as_str(),
        "retry-correlation"
    );
    assert_eq!(
        replay.events()[0]
            .causation_id()
            .expect("stored causation")
            .as_str(),
        "retry-causation"
    );
    let conflicting_metadata = batch(&stream, "retry-operation", "same", &[b"one", b"two"])
        .with_correlation_id(CorrelationId::new("retry-correlation").expect("correlation ID"))
        .with_causation_id(CausationId::new("changed-causation").expect("changed causation ID"));
    assert_eq!(
        store
            .append(&stream, ExpectedVersion::NoStream, conflicting_metadata)
            .await
            .expect_err("metadata changes must not be an exact retry")
            .kind(),
        EventStoreErrorKind::IdentityConflict
    );
    assert_eq!(
        store
            .load(&stream)
            .await
            .expect("load should succeed")
            .len(),
        3
    );
}

pub async fn identity_conflicts<Store: EventStore>(store: &Store) {
    let stream = stream("identity");
    let original = batch(&stream, "identity-operation", "original", &[b"original"]);
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new("identity-operation").expect("valid operation id"),
        ContentFingerprint::digest("different"),
    );
    store
        .append(&stream, ExpectedVersion::NoStream, original)
        .await
        .expect("initial append should succeed");

    let changed = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![
            NewEvent::new(metadata.event_id(0), "contract-event", 1, b"changed")
                .expect("valid event"),
        ],
    )
    .expect("non-empty batch");
    let error = store
        .append(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            changed,
        )
        .await
        .expect_err("same operation identity with changed content must fail");
    assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
    assert_eq!(
        store
            .load(&stream)
            .await
            .expect("load should succeed")
            .len(),
        1
    );

    let other_metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new("other-identity-operation").expect("valid operation id"),
        ContentFingerprint::digest("other"),
    );
    let reused_event = EventBatch::new(
        other_metadata.commit_id().clone(),
        other_metadata.operation_id().clone(),
        other_metadata.operation_fingerprint(),
        vec![
            NewEvent::new(metadata.event_id(0), "contract-event", 1, b"original")
                .expect("valid event"),
        ],
    )
    .expect("non-empty batch");
    let error = store
        .append(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            reused_event,
        )
        .await
        .expect_err("reused event identity must fail");
    assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
}

pub async fn concurrent_append_has_one_winner<Store: EventStore>(store: &Store) {
    let stream = stream("concurrent");
    let first = batch(&stream, "concurrent-a", "a", &[b"a"]);
    let second = batch(&stream, "concurrent-b", "b", &[b"b"]);
    let (first_result, second_result) = tokio::join!(
        store.append(&stream, ExpectedVersion::NoStream, first),
        store.append(&stream, ExpectedVersion::NoStream, second),
    );

    let results = [first_result, second_result];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(error) if error.kind() == EventStoreErrorKind::Conflict))
            .count(),
        1
    );
    assert_eq!(
        store
            .load(&stream)
            .await
            .expect("load should succeed")
            .len(),
        1
    );
}

fn stream(id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new("ContractAggregate").expect("valid aggregate type"),
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
                metadata.event_id(u32::try_from(ordinal).expect("small contract batch")),
                "contract-event",
                1,
                payload.to_vec(),
            )
            .expect("valid contract event")
        })
        .collect();
    EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )
    .expect("non-empty contract batch")
}
