use std::fmt::{self, Display};

use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreError, EventStoreErrorKind, ExecutionMetadata, ExpectedVersion, NewEvent,
    OperationId, StreamId, StreamVersion,
};
use rostfrei_messaging_core::{CausationId, CorrelationId};

#[derive(Debug)]
pub enum ContractTestError {
    Store {
        context: &'static str,
        source: EventStoreError,
    },
    UnexpectedSuccess {
        context: &'static str,
    },
    InvalidFixture {
        context: &'static str,
        message: String,
    },
    MissingExpectedValue {
        context: &'static str,
        expected: &'static str,
    },
    UnexpectedEventCount {
        context: &'static str,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for ContractTestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store { context, source } => {
                write!(
                    formatter,
                    "{context}: event store operation failed: {source}"
                )
            }
            Self::UnexpectedSuccess { context } => {
                write!(
                    formatter,
                    "{context}: event store operation unexpectedly succeeded"
                )
            }
            Self::InvalidFixture { context, message } => {
                write!(formatter, "{context}: invalid contract fixture: {message}")
            }
            Self::MissingExpectedValue { context, expected } => {
                write!(formatter, "{context}: expected {expected}")
            }
            Self::UnexpectedEventCount {
                context,
                expected,
                actual,
            } => write!(
                formatter,
                "{context}: expected {expected} event(s), observed {actual}"
            ),
        }
    }
}

impl std::error::Error for ContractTestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store { source, .. } => Some(source),
            Self::UnexpectedSuccess { .. }
            | Self::InvalidFixture { .. }
            | Self::MissingExpectedValue { .. }
            | Self::UnexpectedEventCount { .. } => None,
        }
    }
}

pub type ContractResult<T = ()> = Result<T, ContractTestError>;

pub async fn run<Factory, Store>(make_store: Factory)
where
    Factory: Fn() -> Store,
    Store: EventStore,
{
    assert_contract_success(try_run(make_store).await);
}

pub async fn try_run<Factory, Store>(make_store: Factory) -> ContractResult
where
    Factory: Fn() -> Store,
    Store: EventStore,
{
    try_empty_load(&make_store()).await?;
    try_no_stream_and_exact_versions(&make_store()).await?;
    try_atomic_ordered_batch(&make_store()).await?;
    try_stream_isolation(&make_store()).await?;
    try_identities_are_stream_scoped(&make_store()).await?;
    try_conflict_leaves_history_unchanged(&make_store()).await?;
    try_exact_retry(&make_store()).await?;
    try_identity_conflicts(&make_store()).await?;
    try_concurrent_append_has_one_winner(&make_store()).await
}

pub async fn empty_load<Store: EventStore>(store: &Store) {
    assert_contract_success(try_empty_load(store).await);
}

pub async fn try_empty_load<Store: EventStore>(store: &Store) -> ContractResult {
    let loaded = store
        .load(&stream("empty")?)
        .await
        .map_err(|source| store_error("empty stream load should succeed", source))?;
    assert!(loaded.is_empty(), "an absent stream must load as empty");
    Ok(())
}

pub async fn no_stream_and_exact_versions<Store: EventStore>(store: &Store) {
    assert_contract_success(try_no_stream_and_exact_versions(store).await);
}

pub async fn try_no_stream_and_exact_versions<Store: EventStore>(store: &Store) -> ContractResult {
    let version_stream = stream("versions")?;
    let first = batch(&version_stream, "versions-1", "one", &[b"first"])?;
    let outcome = store
        .append(&version_stream, ExpectedVersion::NoStream, first)
        .await
        .map_err(|source| store_error("NoStream should append to an absent stream", source))?;
    let event = single_event(outcome.events(), "NoStream append")?;
    assert_eq!(event.stream_version(), StreamVersion::new(1));

    let second = batch(&version_stream, "versions-2", "two", &[b"second"])?;
    let outcome = store
        .append(
            &version_stream,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            second,
        )
        .await
        .map_err(|source| store_error("Exact should append at the current version", source))?;
    let event = single_event(outcome.events(), "Exact append")?;
    assert_eq!(event.stream_version(), StreamVersion::new(2));

    let exact_zero_stream = stream("exact-zero")?;
    let invalid = batch(&exact_zero_stream, "exact-zero", "zero", &[b"event"])?;
    let result = store
        .append(
            &exact_zero_stream,
            ExpectedVersion::Exact(StreamVersion::ZERO),
            invalid,
        )
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "Exact zero must not alias NoStream",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
    Ok(())
}

pub async fn atomic_ordered_batch<Store: EventStore>(store: &Store) {
    assert_contract_success(try_atomic_ordered_batch(store).await);
}

pub async fn try_atomic_ordered_batch<Store: EventStore>(store: &Store) -> ContractResult {
    let stream = stream("atomic")?;
    let outcome = store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            batch(
                &stream,
                "atomic-operation",
                "atomic-content",
                &[b"first", b"second", b"third"],
            )?,
        )
        .await
        .map_err(|source| store_error("multi-event append should succeed", source))?;
    assert!(matches!(outcome, AppendOutcome::Appended(_)));

    let loaded = store
        .load(&stream)
        .await
        .map_err(|source| store_error("atomic stream load should succeed", source))?;
    let [first, second, third] = loaded.as_slice() else {
        return Err(unexpected_event_count(
            "atomic ordered batch",
            3,
            loaded.len(),
        ));
    };
    for (index, event) in loaded.iter().enumerate() {
        let version_offset = u64::try_from(index)
            .map_err(|error| fixture_error("three-event stream version", error))?;
        assert_eq!(
            event.stream_version().value(),
            version_offset.saturating_add(1)
        );
        let ordinal = u32::try_from(index)
            .map_err(|error| fixture_error("three-event commit ordinal", error))?;
        assert_eq!(event.commit_event_ordinal(), ordinal);
        assert_eq!(event.commit_event_count(), 3);
    }
    assert_eq!(first.payload(), b"first");
    assert_eq!(second.payload(), b"second");
    assert_eq!(third.payload(), b"third");
    assert!(loaded.windows(2).all(|pair| match pair {
        [first, second] => first.commit_id() == second.commit_id(),
        _ => false,
    }));
    Ok(())
}

pub async fn stream_isolation<Store: EventStore>(store: &Store) {
    assert_contract_success(try_stream_isolation(store).await);
}

pub async fn try_stream_isolation<Store: EventStore>(store: &Store) -> ContractResult {
    let first_stream = stream("isolated-a")?;
    let second_stream = stream("isolated-b")?;
    store
        .append(
            &first_stream,
            ExpectedVersion::NoStream,
            batch(&first_stream, "isolation-a", "a", &[b"a"])?,
        )
        .await
        .map_err(|source| store_error("first isolated stream append should succeed", source))?;
    store
        .append(
            &second_stream,
            ExpectedVersion::NoStream,
            batch(&second_stream, "isolation-b", "b", &[b"b"])?,
        )
        .await
        .map_err(|source| {
            store_error(
                "second stream must have an independent version gate",
                source,
            )
        })?;

    let first = store
        .load(&first_stream)
        .await
        .map_err(|source| store_error("first isolated stream load should succeed", source))?;
    let second = store
        .load(&second_stream)
        .await
        .map_err(|source| store_error("second isolated stream load should succeed", source))?;
    let first = single_event(&first, "first isolated stream")?;
    let second = single_event(&second, "second isolated stream")?;
    assert_eq!(first.payload(), b"a");
    assert_eq!(second.payload(), b"b");
    Ok(())
}

pub async fn identities_are_stream_scoped<Store: EventStore>(store: &Store) {
    assert_contract_success(try_identities_are_stream_scoped(store).await);
}

pub async fn try_identities_are_stream_scoped<Store: EventStore>(store: &Store) -> ContractResult {
    let first_stream = stream("identity-scope-a")?;
    let second_stream = StreamId::new(
        AggregateType::new("OtherContractAggregate")
            .map_err(|error| fixture_error("other contract aggregate type", error))?,
        AggregateId::new("identity-scope-b")
            .map_err(|error| fixture_error("other contract aggregate ID", error))?,
    );
    store
        .append(
            &first_stream,
            ExpectedVersion::NoStream,
            batch(&first_stream, "shared-operation", "same", &[b"first"])?,
        )
        .await
        .map_err(|source| store_error("first identity-scoped append should succeed", source))?;
    store
        .append(
            &second_stream,
            ExpectedVersion::NoStream,
            batch(&second_stream, "shared-operation", "same", &[b"second"])?,
        )
        .await
        .map_err(|source| store_error("operation identities must be stream scoped", source))?;
    Ok(())
}

pub async fn conflict_leaves_history_unchanged<Store: EventStore>(store: &Store) {
    assert_contract_success(try_conflict_leaves_history_unchanged(store).await);
}

pub async fn try_conflict_leaves_history_unchanged<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let stream = stream("unchanged")?;
    store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            batch(&stream, "unchanged-1", "first", &[b"accepted"])?,
        )
        .await
        .map_err(|source| store_error("initial unchanged-history append should succeed", source))?;
    let before = store
        .load(&stream)
        .await
        .map_err(|source| store_error("pre-conflict history load should succeed", source))?;

    let result = store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            batch(
                &stream,
                "unchanged-2",
                "conflicting",
                &[b"not", b"appended"],
            )?,
        )
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "stale expected version should conflict",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::Conflict);
    let after = store
        .load(&stream)
        .await
        .map_err(|source| store_error("post-conflict history load should succeed", source))?;
    assert_eq!(after, before, "a failed batch must append no prefix");
    Ok(())
}

pub async fn exact_retry<Store: EventStore>(store: &Store) {
    assert_contract_success(try_exact_retry(store).await);
}

pub async fn try_exact_retry<Store: EventStore>(store: &Store) -> ContractResult {
    let stream = stream("retry")?;
    let correlation = CorrelationId::new("retry-correlation")
        .map_err(|error| fixture_error("retry correlation ID", error))?;
    let causation = CausationId::new("retry-causation")
        .map_err(|error| fixture_error("retry causation ID", error))?;
    let retry_batch = batch(&stream, "retry-operation", "same", &[b"one", b"two"])?
        .with_correlation_id(correlation)
        .with_causation_id(causation);
    let first = store
        .append(&stream, ExpectedVersion::NoStream, retry_batch.clone())
        .await
        .map_err(|source| store_error("initial retry append should succeed", source))?;
    assert!(matches!(first, AppendOutcome::Appended(_)));

    let later = batch(&stream, "later-operation", "later", &[b"later"])?;
    store
        .append(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(2)),
            later,
        )
        .await
        .map_err(|source| store_error("later commit should succeed", source))?;

    let replay = store
        .append(&stream, ExpectedVersion::NoStream, retry_batch)
        .await
        .map_err(|source| store_error("exact retry should succeed", source))?;
    assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
    assert_eq!(replay.events(), first.events());
    let Some(replayed_event) = replay.events().first() else {
        return Err(missing("exact retry", "at least one replayed event"));
    };
    let correlation = replayed_event
        .correlation_id()
        .ok_or_else(|| missing("exact retry", "stored correlation ID"))?;
    assert_eq!(correlation.as_str(), "retry-correlation");
    let causation = replayed_event
        .causation_id()
        .ok_or_else(|| missing("exact retry", "stored causation ID"))?;
    assert_eq!(causation.as_str(), "retry-causation");

    let changed_correlation = CorrelationId::new("retry-correlation")
        .map_err(|error| fixture_error("changed retry correlation ID", error))?;
    let changed_causation = CausationId::new("changed-causation")
        .map_err(|error| fixture_error("changed retry causation ID", error))?;
    let conflicting_metadata = batch(&stream, "retry-operation", "same", &[b"one", b"two"])?
        .with_correlation_id(changed_correlation)
        .with_causation_id(changed_causation);
    let result = store
        .append(&stream, ExpectedVersion::NoStream, conflicting_metadata)
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "metadata changes must not be an exact retry",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
    let loaded = store
        .load(&stream)
        .await
        .map_err(|source| store_error("post-retry history load should succeed", source))?;
    assert_eq!(loaded.len(), 3);
    Ok(())
}

pub async fn identity_conflicts<Store: EventStore>(store: &Store) {
    assert_contract_success(try_identity_conflicts(store).await);
}

pub async fn try_identity_conflicts<Store: EventStore>(store: &Store) -> ContractResult {
    let stream = stream("identity")?;
    let original = batch(&stream, "identity-operation", "original", &[b"original"])?;
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new("identity-operation")
            .map_err(|error| fixture_error("identity-conflict operation ID", error))?,
        ContentFingerprint::digest("different"),
    );
    store
        .append(&stream, ExpectedVersion::NoStream, original)
        .await
        .map_err(|source| store_error("initial identity append should succeed", source))?;

    let changed_event = NewEvent::new(metadata.event_id(0), "contract-event", 1, b"changed")
        .map_err(|error| fixture_error("changed identity-conflict event", error))?;
    let changed = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![changed_event],
    )
    .map_err(|error| fixture_error("changed identity-conflict batch", error))?;
    let result = store
        .append(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            changed,
        )
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "changed content with the same operation identity must fail",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
    let loaded = store
        .load(&stream)
        .await
        .map_err(|source| store_error("post-identity-conflict load should succeed", source))?;
    assert_eq!(loaded.len(), 1);

    let other_metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new("other-identity-operation")
            .map_err(|error| fixture_error("other identity operation ID", error))?,
        ContentFingerprint::digest("other"),
    );
    let reused_event = NewEvent::new(metadata.event_id(0), "contract-event", 1, b"original")
        .map_err(|error| fixture_error("reused identity event", error))?;
    let reused_event = EventBatch::new(
        other_metadata.commit_id().clone(),
        other_metadata.operation_id().clone(),
        other_metadata.operation_fingerprint(),
        vec![reused_event],
    )
    .map_err(|error| fixture_error("reused identity batch", error))?;
    let result = store
        .append(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            reused_event,
        )
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "reused event identity must fail",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
    Ok(())
}

pub async fn concurrent_append_has_one_winner<Store: EventStore>(store: &Store) {
    assert_contract_success(try_concurrent_append_has_one_winner(store).await);
}

pub async fn try_concurrent_append_has_one_winner<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let stream = stream("concurrent")?;
    let first = batch(&stream, "concurrent-a", "a", &[b"a"])?;
    let second = batch(&stream, "concurrent-b", "b", &[b"b"])?;
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
    let loaded = store
        .load(&stream)
        .await
        .map_err(|source| store_error("concurrent stream load should succeed", source))?;
    assert_eq!(loaded.len(), 1);
    Ok(())
}

fn stream(id: &str) -> ContractResult<StreamId> {
    let aggregate_type = AggregateType::new("ContractAggregate")
        .map_err(|error| fixture_error("contract aggregate type", error))?;
    let aggregate_id =
        AggregateId::new(id).map_err(|error| fixture_error("contract aggregate ID", error))?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
}

fn batch(
    stream: &StreamId,
    operation_id: &str,
    fingerprint_content: &str,
    payloads: &[&[u8]],
) -> ContractResult<EventBatch> {
    let operation_id = OperationId::new(operation_id)
        .map_err(|error| fixture_error("contract operation ID", error))?;
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        operation_id,
        ContentFingerprint::digest(fingerprint_content),
    );
    let events = payloads
        .iter()
        .enumerate()
        .map(|(ordinal, payload)| {
            let ordinal = u32::try_from(ordinal)
                .map_err(|error| fixture_error("contract event ordinal", error))?;
            NewEvent::new(
                metadata.event_id(ordinal),
                "contract-event",
                1,
                payload.to_vec(),
            )
            .map_err(|error| fixture_error("contract event", error))
        })
        .collect::<ContractResult<Vec<_>>>()?;
    EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )
    .map_err(|error| fixture_error("contract event batch", error))
}

fn assert_contract_success(result: ContractResult) {
    assert_eq!(
        result.map_err(|error| error.to_string()),
        Ok(()),
        "event store contract failed"
    );
}

fn single_event<'a, T>(events: &'a [T], context: &'static str) -> ContractResult<&'a T> {
    let [event] = events else {
        return Err(unexpected_event_count(context, 1, events.len()));
    };
    Ok(event)
}

const fn unexpected_event_count(
    context: &'static str,
    expected: usize,
    actual: usize,
) -> ContractTestError {
    ContractTestError::UnexpectedEventCount {
        context,
        expected,
        actual,
    }
}

fn store_error(context: &'static str, source: EventStoreError) -> ContractTestError {
    ContractTestError::Store { context, source }
}

fn fixture_error(context: &'static str, error: impl Display) -> ContractTestError {
    ContractTestError::InvalidFixture {
        context,
        message: error.to_string(),
    }
}

const fn missing(context: &'static str, expected: &'static str) -> ContractTestError {
    ContractTestError::MissingExpectedValue { context, expected }
}
