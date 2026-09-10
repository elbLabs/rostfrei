#![allow(clippy::panic_in_result_fn)]

use super::*;
use async_trait::async_trait;
use rostfrei_core::{EventHistory, TransactionReceipt};

#[derive(Clone)]
struct SessionStore(NatsEventStore);

#[async_trait]
impl EventHistory for SessionStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.0.load(stream_id).await
    }
}

#[async_trait]
impl EventStore for SessionStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        let session = self.0.append_session().await?;
        session.load(stream_id).await?;
        session.append(stream_id, expected_version, batch).await
    }

    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        let session = self.0.append_session().await?;
        for participant in transaction.participants() {
            session.load(participant.stream_id()).await?;
        }
        session.append_transaction(transaction).await
    }

    async fn load_transaction_receipt(
        &self,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.0.load_transaction_receipt(operation_id).await
    }

    async fn load_transaction_receipt_in_context(
        &self,
        bounded_context: &rostfrei_messaging_core::BoundedContextName,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.0
            .load_transaction_receipt_in_context(bounded_context, operation_id)
            .await
    }
}

async fn setup(label: &str) -> TestResult<(async_nats::jetstream::Context, NatsEventStore)> {
    let url = std::env::var("ROSTFREI_NATS_URL")?;
    let context = connect_context(&url).await?;
    let (bounded_context, stream_name) = unique_names(label)?;
    let config = NatsEventStoreConfig::new(&bounded_context, stream_name)?
        .with_storage_limits(64 * 1024 * 1024, DEFAULT_EVENT_STORE_MAX_EVENT_BYTES)?;
    provision_event_store(&context, &config).await?;
    let store = NatsEventStore::connect(context.clone(), config).await?;
    Ok((context, store))
}

#[tokio::test]
#[ignore = "requires ROSTFREI_NATS_URL"]
async fn sessions_satisfy_direct_and_transaction_contracts() -> TestResult<()> {
    let (context, store) = setup("session-contract").await?;
    event_store_contract::try_run(|| SessionStore(store.clone())).await?;
    event_store_contract::try_run_atomic_multi_stream_transactions(|| SessionStore(store.clone()))
        .await?;
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires ROSTFREI_NATS_URL"]
#[allow(clippy::too_many_lines)]
async fn append_request_count_does_not_grow_with_loaded_history() -> TestResult<()> {
    let (context, store) = setup("session-read-cost").await?;
    let client = context.client();
    let mut direct_costs = Vec::new();
    let mut transaction_costs = Vec::new();
    for size in [1_u32, 100] {
        let writer = stream(&format!("writer-{size}"))?;
        let guard = stream(&format!("guard-{size}"))?;
        let secondary = stream(&format!("secondary-{size}"))?;
        let seed_operation = format!("seed-{size}");
        for participant in [&writer, &guard, &secondary] {
            store
                .append(
                    participant,
                    ExpectedVersion::NoStream,
                    repeated_batch(participant, &seed_operation, &seed_operation, size)?,
                )
                .await?;
        }
        let session = store.append_session().await?;
        assert_eq!(session.load(&writer).await?.len(), usize::try_from(size)?);
        client.flush().await?;
        let before = client.statistics().out_messages.load(Ordering::Relaxed);
        let operation = format!("direct-{size}");
        let outcome = session
            .append(
                &writer,
                ExpectedVersion::Exact(StreamVersion::new(u64::from(size))),
                batch(&writer, &operation, &operation, &[b"new"])?,
            )
            .await?;
        assert!(matches!(outcome, AppendOutcome::Appended(_)));
        client.flush().await?;
        direct_costs.push(
            client
                .statistics()
                .out_messages
                .load(Ordering::Relaxed)
                .checked_sub(before)
                .ok_or("message counter regressed")?,
        );

        let session = store.append_session().await?;
        for participant in [&writer, &guard, &secondary] {
            session.load(participant).await?;
        }
        let operation = format!("transaction-{size}");
        let transaction = EventTransaction::new(
            OperationId::new(&operation)?,
            ContentFingerprint::digest(&operation),
            vec![
                TransactionParticipant::new(
                    guard,
                    ExpectedVersion::Exact(StreamVersion::new(u64::from(size))),
                    None,
                ),
                TransactionParticipant::new(
                    writer.clone(),
                    ExpectedVersion::Exact(StreamVersion::new(checked_add_u64(
                        u64::from(size),
                        1,
                        "writer version",
                    )?)),
                    Some(batch(&writer, &operation, &operation, &[b"one", b"two"])?),
                ),
                TransactionParticipant::new(
                    secondary.clone(),
                    ExpectedVersion::Exact(StreamVersion::new(u64::from(size))),
                    Some(batch(&secondary, &operation, &operation, &[b"three"])?),
                ),
            ],
        );
        client.flush().await?;
        let before = client.statistics().out_messages.load(Ordering::Relaxed);
        let outcome = session.append_transaction(transaction.clone()).await?;
        assert!(matches!(outcome, TransactionAppendOutcome::Appended(_)));
        client.flush().await?;
        transaction_costs.push(
            client
                .statistics()
                .out_messages
                .load(Ordering::Relaxed)
                .checked_sub(before)
                .ok_or("message counter regressed")?,
        );
        assert!(matches!(
            store.append_transaction(transaction).await?,
            TransactionAppendOutcome::ExactReplay(_)
        ));
    }
    assert_eq!(
        direct_costs.first(),
        direct_costs.last(),
        "direct costs: {direct_costs:?}"
    );
    assert_eq!(
        transaction_costs.first(),
        transaction_costs.last(),
        "transaction costs: {transaction_costs:?}"
    );
    assert!(
        direct_costs.iter().all(|count| *count < 20),
        "direct costs: {direct_costs:?}"
    );
    assert!(
        transaction_costs.iter().all(|count| *count < 30),
        "transaction costs: {transaction_costs:?}"
    );
    eprintln!(
        "append requests for histories of 1 / 100 events: direct {direct_costs:?}, transaction {transaction_costs:?}"
    );
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires ROSTFREI_NATS_URL"]
async fn stale_sessions_reconcile_replays_and_preserve_read_guards() -> TestResult<()> {
    let (context, store) = setup("session-races").await?;
    let writer = stream("writer")?;
    let guard = stream("guard")?;
    let stale_writer = store.append_session().await?;
    let stale_replay = store.append_session().await?;
    let stale_identity = store.append_session().await?;
    let stale_guard = store.append_session().await?;
    let winning_batch = batch(&guard, "winner", "winner", &[b"one"])?;
    for session in [&stale_writer, &stale_replay, &stale_identity, &stale_guard] {
        assert!(session.load(&guard).await?.is_empty());
    }
    assert!(stale_guard.load(&writer).await?.is_empty());
    store
        .append(&guard, ExpectedVersion::NoStream, winning_batch.clone())
        .await?;
    let conflict = stale_writer
        .append(
            &guard,
            ExpectedVersion::NoStream,
            batch(&guard, "loser", "loser", &[b"two"])?,
        )
        .await
        .expect_err("stale append must conflict");
    assert_eq!(conflict.kind(), EventStoreErrorKind::Conflict);
    let replay = stale_replay
        .append(&guard, ExpectedVersion::NoStream, winning_batch)
        .await?;
    assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
    let identity = stale_identity
        .append(
            &guard,
            ExpectedVersion::NoStream,
            batch(&guard, "winner", "winner", &[b"changed"])?,
        )
        .await
        .expect_err("different content must conflict");
    assert_eq!(identity.kind(), EventStoreErrorKind::IdentityConflict);

    let transaction = EventTransaction::new(
        OperationId::new("guarded")?,
        ContentFingerprint::digest("guarded"),
        vec![
            TransactionParticipant::new(guard.clone(), ExpectedVersion::NoStream, None),
            TransactionParticipant::new(
                writer.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&writer, "guarded", "guarded", &[b"written"])?),
            ),
        ],
    );
    let conflict = stale_guard
        .append_transaction(transaction)
        .await
        .expect_err("stale read guard must conflict");
    assert_eq!(conflict.kind(), EventStoreErrorKind::Conflict);
    assert!(store.load(&writer).await?.is_empty());
    assert!(
        store
            .load_transaction_receipt(&OperationId::new("guarded")?)
            .await?
            .is_none()
    );
    assert_eq!(store.append_session().await?.load(&guard).await?.len(), 1);
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires ROSTFREI_NATS_URL"]
async fn stream_recreation_invalidates_loaded_sessions() -> TestResult<()> {
    let (context, store) = setup("session-reset").await?;
    let stream_id = stream("aggregate")?;
    let stale = store.append_session().await?;
    let stale_transaction = store.append_session().await?;
    assert!(stale.load(&stream_id).await?.is_empty());
    assert!(stale_transaction.load(&stream_id).await?.is_empty());
    context.delete_stream(store.config().stream_name()).await?;
    provision_event_store(&context, store.config()).await?;
    let new_batch = batch(&stream_id, "write", "write", &[b"new"])?;
    let result = stale
        .append(&stream_id, ExpectedVersion::NoStream, new_batch.clone())
        .await;
    assert!(
        matches!(result, Err(error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    let transaction = EventTransaction::new(
        OperationId::new("write")?,
        ContentFingerprint::digest("write"),
        vec![TransactionParticipant::new(
            stream_id.clone(),
            ExpectedVersion::NoStream,
            Some(new_batch.clone()),
        )],
    );
    let result = stale_transaction.append_transaction(transaction).await;
    assert!(
        matches!(result, Err(error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    assert!(store.load(&stream_id).await?.is_empty());
    let fresh = store.append_session().await?;
    assert!(fresh.load(&stream_id).await?.is_empty());
    assert!(matches!(
        fresh
            .append(&stream_id, ExpectedVersion::NoStream, new_batch)
            .await?,
        AppendOutcome::Appended(_)
    ));
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}
