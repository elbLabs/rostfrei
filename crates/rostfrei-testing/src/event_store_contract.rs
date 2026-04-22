use std::fmt::{self, Display};

use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreError, EventStoreErrorKind, EventTransaction, ExecutionMetadata, ExpectedVersion,
    MAX_TRANSACTION_ITEMS, NewEvent, OperationId, StreamId, StreamVersion,
    TransactionAppendOutcome, TransactionParticipant,
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
    try_multi_stream_transaction_is_atomic_and_ordered(&make_store()).await?;
    try_transaction_rejects_a_read_only_primary(&make_store()).await?;
    try_transaction_accepts_a_read_only_participant(&make_store()).await?;
    try_transaction_identities_are_primary_stream_scoped(&make_store()).await?;
    try_transaction_rejects_primary_identity_reused_from_a_participant(&make_store()).await?;
    try_multi_stream_conflict_leaves_all_histories_unchanged(&make_store()).await?;
    try_multi_stream_exact_retry_survives_later_commits(&make_store()).await?;
    try_transaction_item_limit_preserves_direct_append_limit(&make_store()).await?;
    try_read_guards_reduce_the_transaction_event_allowance(&make_store()).await?;
    try_concurrent_transactions_have_one_winner(&make_store()).await?;
    try_exact_retry(&make_store()).await?;
    try_identity_conflicts(&make_store()).await?;
    try_concurrent_append_has_one_winner(&make_store()).await
}

pub async fn transaction_identities_are_primary_stream_scoped<Store: EventStore>(store: &Store) {
    assert_contract_success(try_transaction_identities_are_primary_stream_scoped(store).await);
}

pub async fn try_transaction_identities_are_primary_stream_scoped<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let legacy_stream = stream("transaction-scope-legacy")?;
    let first_primary = stream("transaction-scope-primary-a")?;
    let first_secondary = stream("transaction-scope-secondary-a")?;
    let second_primary = stream("transaction-scope-primary-b")?;
    let second_secondary = stream("transaction-scope-secondary-b")?;
    let operation = "transaction-shared-operation";
    let operation_id = OperationId::new(operation)
        .map_err(|error| fixture_error("transaction-scoped operation ID", error))?;
    let fingerprint = ContentFingerprint::digest(operation);
    let legacy_batch = batch(&legacy_stream, operation, operation, &[b"legacy"])?;
    let legacy = store
        .append(
            &legacy_stream,
            ExpectedVersion::NoStream,
            legacy_batch.clone(),
        )
        .await
        .map_err(|source| store_error("legacy transaction-scope append", source))?;

    for (primary, secondary, primary_payload, secondary_payload) in [
        (
            &first_primary,
            &first_secondary,
            b"a".as_slice(),
            b"b".as_slice(),
        ),
        (
            &second_primary,
            &second_secondary,
            b"c".as_slice(),
            b"d".as_slice(),
        ),
    ] {
        store
            .append_transaction(EventTransaction::new(
                operation_id.clone(),
                fingerprint,
                vec![
                    TransactionParticipant::new(
                        primary.clone(),
                        ExpectedVersion::NoStream,
                        Some(batch(primary, operation, operation, &[primary_payload])?),
                    ),
                    TransactionParticipant::new(
                        secondary.clone(),
                        ExpectedVersion::NoStream,
                        Some(batch(
                            secondary,
                            operation,
                            operation,
                            &[secondary_payload],
                        )?),
                    ),
                ],
            ))
            .await
            .map_err(|source| store_error("primary-stream-scoped transaction append", source))?;
    }

    assert!(
        store
            .load_transaction_receipt(&first_primary, &operation_id)
            .await
            .map_err(|source| store_error("first transaction receipt lookup", source))?
            .is_some()
    );
    assert!(
        store
            .load_transaction_receipt(&legacy_stream, &operation_id)
            .await
            .map_err(|source| store_error("legacy transaction receipt lookup", source))?
            .is_none()
    );
    let legacy_replay = store
        .append(&legacy_stream, ExpectedVersion::NoStream, legacy_batch)
        .await
        .map_err(|source| store_error("legacy transaction-scope exact replay", source))?;
    assert!(legacy_replay.is_exact_replay());
    assert_eq!(legacy_replay.events(), legacy.events());
    Ok(())
}

pub async fn concurrent_transactions_have_one_winner<Store: EventStore>(store: &Store) {
    assert_contract_success(try_concurrent_transactions_have_one_winner(store).await);
}

pub async fn try_concurrent_transactions_have_one_winner<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let shared_stream = stream("transaction-concurrent-shared")?;
    let first_observed = stream("transaction-concurrent-observed-a")?;
    let second_observed = stream("transaction-concurrent-observed-b")?;
    let first_operation = "transaction-concurrent-a";
    let second_operation = "transaction-concurrent-b";
    let first = EventTransaction::new(
        OperationId::new(first_operation)
            .map_err(|error| fixture_error("first concurrent transaction operation ID", error))?,
        ContentFingerprint::digest(first_operation),
        vec![
            TransactionParticipant::new(
                shared_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch(
                    &shared_stream,
                    first_operation,
                    first_operation,
                    &[b"a"],
                )?),
            ),
            TransactionParticipant::new(first_observed, ExpectedVersion::NoStream, None),
        ],
    );
    let second = EventTransaction::new(
        OperationId::new(second_operation)
            .map_err(|error| fixture_error("second concurrent transaction operation ID", error))?,
        ContentFingerprint::digest(second_operation),
        vec![
            TransactionParticipant::new(
                shared_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch(
                    &shared_stream,
                    second_operation,
                    second_operation,
                    &[b"b"],
                )?),
            ),
            TransactionParticipant::new(second_observed, ExpectedVersion::NoStream, None),
        ],
    );
    let results: [_; 2] = tokio::join!(
        store.append_transaction(first),
        store.append_transaction(second),
    )
    .into();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let error = results
        .into_iter()
        .find_map(Result::err)
        .ok_or_else(|| missing("concurrent transactions", "one conflicting transaction"))?;
    assert_eq!(error.kind(), EventStoreErrorKind::Conflict);
    let loaded = store
        .load(&shared_stream)
        .await
        .map_err(|source| store_error("concurrent transaction stream load", source))?;
    assert_eq!(loaded.len(), 1);
    Ok(())
}

pub async fn transaction_rejects_a_read_only_primary<Store: EventStore>(store: &Store) {
    assert_contract_success(try_transaction_rejects_a_read_only_primary(store).await);
}

pub async fn try_transaction_rejects_a_read_only_primary<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let primary = stream("transaction-read-only-primary")?;
    let changed = stream("transaction-read-only-primary-changed")?;
    let operation = "transaction-read-only-primary";
    let result = store
        .append_transaction(EventTransaction::new(
            OperationId::new(operation)
                .map_err(|error| fixture_error("read-only primary operation ID", error))?,
            ContentFingerprint::digest(operation),
            vec![
                TransactionParticipant::new(primary.clone(), ExpectedVersion::NoStream, None),
                TransactionParticipant::new(
                    changed.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch(
                        &changed,
                        operation,
                        operation,
                        &[b"must-not-append"],
                    )?),
                ),
            ],
        ))
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "transaction with a read-only primary participant",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
    for participant in [&primary, &changed] {
        let history = store
            .load(participant)
            .await
            .map_err(|source| store_error("read-only primary participant load", source))?;
        assert!(history.is_empty());
    }
    Ok(())
}

pub async fn transaction_accepts_a_read_only_participant<Store: EventStore>(store: &Store) {
    assert_contract_success(try_transaction_accepts_a_read_only_participant(store).await);
}

#[allow(clippy::too_many_lines)]
pub async fn try_transaction_accepts_a_read_only_participant<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let changed_stream = stream("transaction-write-participant")?;
    let observed_stream = stream("transaction-read-participant")?;
    let operation = "transaction-read-only";
    let transaction = EventTransaction::new(
        OperationId::new(operation)
            .map_err(|error| fixture_error("read-only transaction operation ID", error))?,
        ContentFingerprint::digest(operation),
        vec![
            TransactionParticipant::new(
                changed_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&changed_stream, operation, operation, &[b"changed"])?),
            ),
            TransactionParticipant::new(observed_stream.clone(), ExpectedVersion::NoStream, None),
        ],
    );
    let outcome = store
        .append_transaction(transaction.clone())
        .await
        .map_err(|source| store_error("read-only participant transaction append", source))?;
    assert_eq!(outcome.receipt().events().len(), 1);
    let [_, observed_receipt] = outcome.receipt().streams() else {
        return Err(unexpected_event_count(
            "read-only participant transaction receipts",
            2,
            outcome.receipt().streams().len(),
        ));
    };
    assert_eq!(observed_receipt.base_version(), StreamVersion::ZERO);
    assert!(observed_receipt.events().is_empty());
    let observed_history = store
        .load(&observed_stream)
        .await
        .map_err(|source| store_error("read-only participant stream load", source))?;
    assert!(observed_history.is_empty());
    store
        .append(
            &observed_stream,
            ExpectedVersion::NoStream,
            batch(
                &observed_stream,
                operation,
                operation,
                &[b"later-independent-operation"],
            )?,
        )
        .await
        .map_err(|source| store_error("post-observation independent append", source))?;
    let replay = store
        .append_transaction(transaction)
        .await
        .map_err(|source| store_error("read-only participant transaction replay", source))?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.receipt().events(), outcome.receipt().events());

    let prior_observed_stream = stream("transaction-prior-read-participant")?;
    let later_changed_stream = stream("transaction-later-write-participant")?;
    let prior_operation = "transaction-prior-read-only";
    store
        .append(
            &prior_observed_stream,
            ExpectedVersion::NoStream,
            batch(
                &prior_observed_stream,
                prior_operation,
                prior_operation,
                &[b"prior-independent-operation"],
            )?,
        )
        .await
        .map_err(|source| store_error("prior observed-stream append", source))?;
    let prior_outcome = store
        .append_transaction(EventTransaction::new(
            OperationId::new(prior_operation)
                .map_err(|error| fixture_error("prior read-only operation ID", error))?,
            ContentFingerprint::digest(prior_operation),
            vec![
                TransactionParticipant::new(
                    later_changed_stream.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch(
                        &later_changed_stream,
                        prior_operation,
                        prior_operation,
                        &[b"changed-after-observation"],
                    )?),
                ),
                TransactionParticipant::new(
                    prior_observed_stream,
                    ExpectedVersion::Exact(StreamVersion::new(1)),
                    None,
                ),
            ],
        ))
        .await
        .map_err(|source| store_error("prior read-only participant transaction", source))?;
    assert_eq!(prior_outcome.receipt().events().len(), 1);
    let [_, prior_observed_receipt] = prior_outcome.receipt().streams() else {
        return Err(unexpected_event_count(
            "prior read-only participant transaction receipts",
            2,
            prior_outcome.receipt().streams().len(),
        ));
    };
    assert_eq!(prior_observed_receipt.base_version(), StreamVersion::new(1));
    Ok(())
}

pub async fn transaction_rejects_primary_identity_reused_from_a_participant<Store: EventStore>(
    store: &Store,
) {
    assert_contract_success(
        try_transaction_rejects_primary_identity_reused_from_a_participant(store).await,
    );
}

pub async fn try_transaction_rejects_primary_identity_reused_from_a_participant<
    Store: EventStore,
>(
    store: &Store,
) -> ContractResult {
    let original_primary = stream("transaction-reused-participant-original-primary")?;
    let reused_primary = stream("transaction-reused-participant-new-primary")?;
    let untouched = stream("transaction-reused-participant-untouched")?;
    let operation = "transaction-reused-participant-operation";
    let operation_id = OperationId::new(operation)
        .map_err(|error| fixture_error("reused participant operation ID", error))?;
    let fingerprint = ContentFingerprint::digest(operation);
    let reused_primary_batch = batch(
        &reused_primary,
        operation,
        operation,
        &[b"original-participant-write"],
    )?;
    store
        .append_transaction(EventTransaction::new(
            operation_id.clone(),
            fingerprint,
            vec![
                TransactionParticipant::new(
                    original_primary.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch(
                        &original_primary,
                        operation,
                        operation,
                        &[b"original-primary-write"],
                    )?),
                ),
                TransactionParticipant::new(
                    reused_primary.clone(),
                    ExpectedVersion::NoStream,
                    Some(reused_primary_batch.clone()),
                ),
            ],
        ))
        .await
        .map_err(|source| store_error("transaction with reusable participant", source))?;
    let before = store
        .load(&reused_primary)
        .await
        .map_err(|source| store_error("reused participant pre-conflict load", source))?;

    let result = store
        .append_transaction(EventTransaction::new(
            operation_id,
            fingerprint,
            vec![
                TransactionParticipant::new(
                    reused_primary.clone(),
                    ExpectedVersion::Exact(StreamVersion::new(1)),
                    Some(reused_primary_batch),
                ),
                TransactionParticipant::new(
                    untouched.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch(
                        &untouched,
                        operation,
                        operation,
                        &[b"must-not-append"],
                    )?),
                ),
            ],
        ))
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "transaction primary identity reused from a prior participant",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
    let after = store
        .load(&reused_primary)
        .await
        .map_err(|source| store_error("reused participant post-conflict load", source))?;
    assert_eq!(after, before);
    let untouched_history = store
        .load(&untouched)
        .await
        .map_err(|source| store_error("reused participant untouched-stream load", source))?;
    assert!(untouched_history.is_empty());
    Ok(())
}

pub async fn multi_stream_transaction_is_atomic_and_ordered<Store: EventStore>(store: &Store) {
    assert_contract_success(try_multi_stream_transaction_is_atomic_and_ordered(store).await);
}

pub async fn try_multi_stream_transaction_is_atomic_and_ordered<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let first_stream = stream("transaction-atomic-a")?;
    let second_stream = stream("transaction-atomic-b")?;
    let operation = "transaction-atomic";
    let transaction = EventTransaction::new(
        OperationId::new(operation)
            .map_err(|error| fixture_error("atomic transaction operation ID", error))?,
        ContentFingerprint::digest(operation),
        vec![
            TransactionParticipant::new(
                first_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch(
                    &first_stream,
                    operation,
                    operation,
                    &[b"a-1", b"a-2"],
                )?),
            ),
            TransactionParticipant::new(
                second_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&second_stream, operation, operation, &[b"b-1"])?),
            ),
        ],
    );
    let outcome = store
        .append_transaction(transaction)
        .await
        .map_err(|source| store_error("atomic multi-stream transaction append", source))?;
    assert!(matches!(outcome, TransactionAppendOutcome::Appended(_)));
    let events = outcome.receipt().events();
    let [first, second, third] = events.as_slice() else {
        return Err(unexpected_event_count(
            "atomic multi-stream transaction",
            3,
            events.len(),
        ));
    };
    assert_eq!(first.stream_id(), &first_stream);
    assert_eq!(second.stream_id(), &first_stream);
    assert_eq!(third.stream_id(), &second_stream);
    assert_eq!(first.stream_version(), StreamVersion::new(1));
    assert_eq!(second.stream_version(), StreamVersion::new(2));
    assert_eq!(third.stream_version(), StreamVersion::new(1));
    Ok(())
}

pub async fn multi_stream_conflict_leaves_all_histories_unchanged<Store: EventStore>(
    store: &Store,
) {
    assert_contract_success(try_multi_stream_conflict_leaves_all_histories_unchanged(store).await);
}

pub async fn try_multi_stream_conflict_leaves_all_histories_unchanged<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let changed_stream = stream("transaction-conflict-changed")?;
    let untouched_stream = stream("transaction-conflict-untouched")?;
    store
        .append(
            &changed_stream,
            ExpectedVersion::NoStream,
            batch(
                &changed_stream,
                "transaction-conflict-seed",
                "seed",
                &[b"seed"],
            )?,
        )
        .await
        .map_err(|source| store_error("multi-stream conflict seed append", source))?;
    let before = store
        .load(&changed_stream)
        .await
        .map_err(|source| store_error("pre-conflict changed-stream load", source))?;
    let operation = "transaction-conflict";
    let result = store
        .append_transaction(EventTransaction::new(
            OperationId::new(operation)
                .map_err(|error| fixture_error("conflicting transaction operation ID", error))?,
            ContentFingerprint::digest(operation),
            vec![
                TransactionParticipant::new(
                    untouched_stream.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch(
                        &untouched_stream,
                        operation,
                        operation,
                        &[b"must-not-append"],
                    )?),
                ),
                TransactionParticipant::new(
                    changed_stream.clone(),
                    ExpectedVersion::NoStream,
                    None,
                ),
            ],
        ))
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "stale read participant should conflict",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::Conflict);
    let changed_after = store
        .load(&changed_stream)
        .await
        .map_err(|source| store_error("post-conflict changed-stream load", source))?;
    assert_eq!(changed_after, before);
    let untouched_after = store
        .load(&untouched_stream)
        .await
        .map_err(|source| store_error("post-conflict untouched-stream load", source))?;
    assert!(untouched_after.is_empty());
    Ok(())
}

pub async fn multi_stream_exact_retry_survives_later_commits<Store: EventStore>(store: &Store) {
    assert_contract_success(try_multi_stream_exact_retry_survives_later_commits(store).await);
}

pub async fn try_multi_stream_exact_retry_survives_later_commits<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let first_stream = stream("transaction-retry-a")?;
    let second_stream = stream("transaction-retry-b")?;
    let operation = "transaction-retry";
    let transaction = EventTransaction::new(
        OperationId::new(operation)
            .map_err(|error| fixture_error("transaction retry operation ID", error))?,
        ContentFingerprint::digest(operation),
        vec![
            TransactionParticipant::new(
                first_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&first_stream, operation, operation, &[b"first"])?),
            ),
            TransactionParticipant::new(
                second_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&second_stream, operation, operation, &[b"second"])?),
            ),
        ],
    );
    let first = store
        .append_transaction(transaction.clone())
        .await
        .map_err(|source| store_error("initial multi-stream transaction", source))?;
    for (stream, suffix) in [(&first_stream, "a"), (&second_stream, "b")] {
        store
            .append(
                stream,
                ExpectedVersion::Exact(StreamVersion::new(1)),
                batch(
                    stream,
                    &format!("transaction-retry-later-{suffix}"),
                    suffix,
                    &[b"later"],
                )?,
            )
            .await
            .map_err(|source| store_error("later post-transaction append", source))?;
    }
    let replay = store
        .append_transaction(transaction.clone())
        .await
        .map_err(|source| store_error("multi-stream transaction exact replay", source))?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.receipt().events(), first.receipt().events());

    let changed_preconditions = EventTransaction::new(
        transaction.operation_id().clone(),
        transaction.operation_fingerprint(),
        transaction
            .participants()
            .iter()
            .map(|participant| {
                TransactionParticipant::new(
                    participant.stream_id().clone(),
                    ExpectedVersion::Exact(StreamVersion::new(99)),
                    participant.batch().cloned(),
                )
            })
            .collect(),
    );
    let replay = store
        .append_transaction(changed_preconditions)
        .await
        .map_err(|source| store_error("changed-precondition transaction replay", source))?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.receipt().events(), first.receipt().events());
    Ok(())
}

pub async fn transaction_item_limit_preserves_direct_append_limit<Store: EventStore>(
    store: &Store,
) {
    assert_contract_success(try_transaction_item_limit_preserves_direct_append_limit(store).await);
}

pub async fn try_transaction_item_limit_preserves_direct_append_limit<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let accepted_stream = stream("transaction-item-limit-accepted")?;
    let accepted_operation = "transaction-item-limit-accepted";
    let accepted_event_count = MAX_TRANSACTION_ITEMS.saturating_sub(1);
    let accepted = store
        .append_transaction(EventTransaction::new(
            OperationId::new(accepted_operation)
                .map_err(|error| fixture_error("accepted limit operation ID", error))?,
            ContentFingerprint::digest(accepted_operation),
            vec![TransactionParticipant::new(
                accepted_stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch_with_event_count(
                    &accepted_stream,
                    accepted_operation,
                    accepted_event_count,
                )?),
            )],
        ))
        .await
        .map_err(|source| store_error("accepted transaction item limit", source))?;
    assert_eq!(accepted.receipt().events().len(), accepted_event_count);

    let rejected_stream = stream("transaction-item-limit-rejected")?;
    let rejected_operation = "transaction-item-limit-rejected";
    let maximum_batch =
        batch_with_event_count(&rejected_stream, rejected_operation, MAX_TRANSACTION_ITEMS)?;
    let result = store
        .append_transaction(EventTransaction::new(
            OperationId::new(rejected_operation)
                .map_err(|error| fixture_error("rejected limit operation ID", error))?,
            ContentFingerprint::digest(rejected_operation),
            vec![TransactionParticipant::new(
                rejected_stream.clone(),
                ExpectedVersion::NoStream,
                Some(maximum_batch.clone()),
            )],
        ))
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "transaction exceeding the item limit",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
    let rejected_history = store
        .load(&rejected_stream)
        .await
        .map_err(|source| store_error("rejected transaction stream load", source))?;
    assert!(rejected_history.is_empty());

    let direct = store
        .append(&rejected_stream, ExpectedVersion::NoStream, maximum_batch)
        .await
        .map_err(|source| store_error("maximum-size direct append", source))?;
    assert_eq!(direct.events().len(), MAX_TRANSACTION_ITEMS);
    Ok(())
}

pub async fn read_guards_reduce_the_transaction_event_allowance<Store: EventStore>(store: &Store) {
    assert_contract_success(try_read_guards_reduce_the_transaction_event_allowance(store).await);
}

pub async fn try_read_guards_reduce_the_transaction_event_allowance<Store: EventStore>(
    store: &Store,
) -> ContractResult {
    let accepted_stream = stream("transaction-guard-limit-accepted")?;
    let accepted_guard_a = stream("transaction-guard-limit-accepted-a")?;
    let accepted_guard_b = stream("transaction-guard-limit-accepted-b")?;
    let accepted_operation = "transaction-guard-limit-accepted";
    let accepted_event_count = MAX_TRANSACTION_ITEMS.saturating_sub(3);
    let accepted = store
        .append_transaction(EventTransaction::new(
            OperationId::new(accepted_operation)
                .map_err(|error| fixture_error("accepted guard-limit operation ID", error))?,
            ContentFingerprint::digest(accepted_operation),
            vec![
                TransactionParticipant::new(
                    accepted_stream.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch_with_event_count(
                        &accepted_stream,
                        accepted_operation,
                        accepted_event_count,
                    )?),
                ),
                TransactionParticipant::new(accepted_guard_a, ExpectedVersion::NoStream, None),
                TransactionParticipant::new(accepted_guard_b, ExpectedVersion::NoStream, None),
            ],
        ))
        .await
        .map_err(|source| store_error("accepted read-guard item limit", source))?;
    assert_eq!(accepted.receipt().events().len(), accepted_event_count);

    let rejected_stream = stream("transaction-guard-limit-rejected")?;
    let rejected_guard_a = stream("transaction-guard-limit-rejected-a")?;
    let rejected_guard_b = stream("transaction-guard-limit-rejected-b")?;
    let rejected_operation = "transaction-guard-limit-rejected";
    let rejected_event_count = MAX_TRANSACTION_ITEMS.saturating_sub(2);
    let result = store
        .append_transaction(EventTransaction::new(
            OperationId::new(rejected_operation)
                .map_err(|error| fixture_error("rejected guard-limit operation ID", error))?,
            ContentFingerprint::digest(rejected_operation),
            vec![
                TransactionParticipant::new(
                    rejected_stream.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch_with_event_count(
                        &rejected_stream,
                        rejected_operation,
                        rejected_event_count,
                    )?),
                ),
                TransactionParticipant::new(rejected_guard_a, ExpectedVersion::NoStream, None),
                TransactionParticipant::new(rejected_guard_b, ExpectedVersion::NoStream, None),
            ],
        ))
        .await;
    let Err(error) = result else {
        return Err(ContractTestError::UnexpectedSuccess {
            context: "transaction exceeding the read-guard-adjusted item limit",
        });
    };
    assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
    let rejected_history = store
        .load(&rejected_stream)
        .await
        .map_err(|source| store_error("rejected read-guard stream load", source))?;
    assert!(rejected_history.is_empty());
    Ok(())
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
    let results: [_; 2] = tokio::join!(
        store.append(&stream, ExpectedVersion::NoStream, first),
        store.append(&stream, ExpectedVersion::NoStream, second),
    )
    .into();
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

const fn single_event<'a, T>(events: &'a [T], context: &'static str) -> ContractResult<&'a T> {
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

const fn store_error(context: &'static str, source: EventStoreError) -> ContractTestError {
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

fn batch_with_event_count(
    stream: &StreamId,
    operation_id: &str,
    event_count: usize,
) -> ContractResult<EventBatch> {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation_id)
            .map_err(|error| fixture_error("transaction-limit operation ID", error))?,
        ContentFingerprint::digest(operation_id),
    );
    let events = (0..event_count)
        .map(|ordinal| {
            let ordinal = u32::try_from(ordinal)
                .map_err(|error| fixture_error("transaction-limit event ordinal", error))?;
            NewEvent::new(metadata.event_id(ordinal), "contract-event", 1, b"event")
                .map_err(|error| fixture_error("transaction-limit event", error))
        })
        .collect::<ContractResult<Vec<_>>>()?;
    EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )
    .map_err(|error| fixture_error("transaction-limit event batch", error))
}
