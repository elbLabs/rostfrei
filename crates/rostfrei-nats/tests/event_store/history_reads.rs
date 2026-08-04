#![allow(clippy::panic_in_result_fn)]

use super::*;
use std::time::Duration;

#[tokio::test]
async fn ordinary_history_reads_do_not_traverse_historical_receipts() -> TestResult<()> {
    let (context, store) = directory_fixture("history-read-cost").await?;
    let client = context.client();
    for size in [1_u64, 100] {
        let writer = stream(&format!("history-{size}"))?;
        for index in 0..size {
            let operation = format!("seed-{size}-{index}");
            let expected = if index == 0 {
                ExpectedVersion::NoStream
            } else {
                ExpectedVersion::Exact(StreamVersion::new(index))
            };
            store
                .append_transaction(EventTransaction::new(
                    OperationId::new(&operation)?,
                    ContentFingerprint::digest(&operation),
                    vec![TransactionParticipant::new(
                        writer.clone(),
                        expected,
                        Some(batch(&writer, &operation, &operation, &[b"event"])?),
                    )],
                ))
                .await?;
        }

        client.flush().await?;
        let before = client.statistics().out_messages.load(Ordering::Relaxed);
        let loaded = store.load(&writer).await?;
        client.flush().await?;
        let requests = client
            .statistics()
            .out_messages
            .load(Ordering::Relaxed)
            .checked_sub(before)
            .ok_or("request counter regressed")?;
        assert_eq!(loaded.len(), usize::try_from(size)?);
        // Only this aggregate's event reads, metadata, and bounded window probes.
        // Historical receipt work belongs to the explicit audit below.
        let budget = size.checked_add(16).ok_or("request budget overflowed")?;
        assert!(
            requests <= budget,
            "history of {size} transactions used {requests} requests; budget {budget}"
        );
        eprintln!("history of {size} transactions: {requests} requests");
        let before_audit = client.statistics().out_messages.load(Ordering::Relaxed);
        assert_eq!(store.audit_history(&writer).await?, loaded);
        client.flush().await?;
        let audit_requests = client
            .statistics()
            .out_messages
            .load(Ordering::Relaxed)
            .checked_sub(before_audit)
            .ok_or("audit counter regressed")?;
        assert!(
            audit_requests
                >= requests
                    .checked_add(size.checked_mul(2).ok_or("audit budget overflow")?)
                    .ok_or("audit budget overflow")?
        );
        eprintln!("audit of {size} transactions: {audit_requests} requests");
        let strict = store.clone().with_history_auditing(true);
        for explicit in [false, true] {
            let before = client.statistics().out_messages.load(Ordering::Relaxed);
            let strict_history = if explicit {
                strict.audit_history(&writer).await?
            } else {
                strict.load(&writer).await?
            };
            assert_eq!(strict_history, loaded);
            client.flush().await?;
            let used = client
                .statistics()
                .out_messages
                .load(Ordering::Relaxed)
                .checked_sub(before)
                .ok_or("strict audit counter regressed")?;
            assert_eq!(
                used, audit_requests,
                "strict and explicit auditing must each audit once"
            );
        }
    }
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
async fn auditing_policy_is_local_to_the_handle_and_rejects_bad_receipts_before_writes()
-> TestResult<()> {
    use rostfrei_core::EventHistory;

    for invalid_writes in [false, true] {
        let (context, store) = directory_fixture("history-auditing-policy").await?;
        let aggregate = publish_schema_four_event_without_receipt(&context, store.config()).await?;
        assert!(!store.history_auditing_enabled());
        let ordinary = store.load(&aggregate).await?;
        let ordinary_session = store.append_session().await?;
        assert_eq!(ordinary_session.load(&aggregate).await?, ordinary);

        let strict = store.clone().with_history_auditing(true);
        assert!(strict.history_auditing_enabled());
        assert!(!store.history_auditing_enabled());
        let history: Arc<dyn EventHistory> = Arc::new(strict.clone());
        assert!(
            matches!(history.load(&aggregate).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );
        assert!(
            matches!(strict.list_streams(aggregate.aggregate_type()).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );
        let strict_session = strict.append_session().await?;
        for _ in 0..2 {
            assert!(
                matches!(strict_session.load(&aggregate).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
            );
        }

        let fast = strict.clone().with_history_auditing(false);
        assert!(!fast.history_auditing_enabled());
        assert!(strict.history_auditing_enabled());
        assert_eq!(fast.load(&aggregate).await?, ordinary);
        assert_eq!(ordinary_session.load(&aggregate).await?, ordinary);
        assert!(
            matches!(strict_session.load(&aggregate).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );
        assert!(
            matches!(fast.audit_history(&aggregate).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );
        assert!(
            matches!(fast.audit_streams(aggregate.aggregate_type()).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );

        let before = context
            .get_stream(store.config().stream_name())
            .await?
            .cached_info()
            .state
            .last_sequence;
        let direct = batch(
            &aggregate,
            "strict-direct",
            "strict-direct",
            &[b"must-not-write"],
        )?;
        assert!(
            matches!(strict.append(&aggregate, ExpectedVersion::Exact(StreamVersion::new(1)), direct).await,
            Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );
        let valid = stream("strict-valid-writer")?;
        let operation = "strict-transaction";
        let invalid_batch = if invalid_writes {
            Some(batch(&aggregate, operation, operation, &[b"bad-write"])?)
        } else {
            None
        };
        let transaction = EventTransaction::new(
            OperationId::new(operation)?,
            ContentFingerprint::digest(operation),
            vec![
                TransactionParticipant::new(
                    valid.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch(&valid, operation, operation, &[b"valid-write"])?),
                ),
                TransactionParticipant::new(
                    aggregate.clone(),
                    ExpectedVersion::Exact(StreamVersion::new(1)),
                    invalid_batch,
                ),
            ],
        );
        assert!(
            matches!(strict.append_transaction(transaction.clone()).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );
        assert!(
            matches!(strict_session.append_transaction(transaction).await, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory)
        );
        assert!(store.load(&valid).await?.is_empty());
        assert!(
            store
                .load_transaction_receipt(&OperationId::new(operation)?)
                .await?
                .is_none()
        );
        let after = context
            .get_stream(store.config().stream_name())
            .await?
            .cached_info()
            .state
            .last_sequence;
        assert_eq!(after, before, "strict history audit failure published data");
        context.delete_stream(store.config().stream_name()).await?;
    }
    Ok(())
}

#[tokio::test]
async fn history_reads_preserve_commits_across_sparse_ranges() -> TestResult<()> {
    let (context, store) = directory_fixture("history-read-ranges").await?;
    let writer = stream("range-writer")?;
    let secondary = stream("range-secondary")?;
    let guard = stream("range-empty-guard")?;
    let mut expected = Vec::new();
    // The first window exceeds the raw-read byte budget, exercising continuation
    // before the later windows may be folded into this aggregate's history.
    let payload = vec![b'x'; 12 * 1024];
    let payloads = vec![payload.as_slice(); 40];
    for (operation, expected_version) in [
        ("first", ExpectedVersion::NoStream),
        ("second", ExpectedVersion::Exact(StreamVersion::new(40))),
    ] {
        let outcome = store
            .append(
                &writer,
                expected_version,
                batch(&writer, operation, operation, &payloads)?,
            )
            .await?;
        let AppendOutcome::Appended(events) = outcome else {
            return Err("seed was not appended".into());
        };
        expected.extend(events);
        if operation == "first" {
            let unrelated = stream("unrelated")?;
            store
                .append(
                    &unrelated,
                    ExpectedVersion::NoStream,
                    repeated_batch(&unrelated, "noise", "noise", 100)?,
                )
                .await?;
        }
    }
    assert_eq!(store.load(&writer).await?, expected);

    let operation = "range-transaction";
    let appended = store
        .append_transaction(EventTransaction::new(
            OperationId::new(operation)?,
            ContentFingerprint::digest(operation),
            vec![
                TransactionParticipant::new(guard, ExpectedVersion::NoStream, None),
                TransactionParticipant::new(
                    writer.clone(),
                    ExpectedVersion::Exact(StreamVersion::new(80)),
                    Some(batch(&writer, operation, operation, &[b"a", b"b", b"c"])?),
                ),
                TransactionParticipant::new(
                    secondary.clone(),
                    ExpectedVersion::NoStream,
                    Some(repeated_batch(&secondary, operation, operation, 7)?),
                ),
            ],
        ))
        .await?;
    let TransactionAppendOutcome::Appended(receipt) = appended else {
        return Err("transaction seed was not appended".into());
    };
    expected.extend(
        receipt
            .events()
            .into_iter()
            .filter(|event| event.stream_id() == &writer),
    );
    assert_eq!(store.load(&writer).await?, expected);
    assert_eq!(
        store.load(&secondary).await?,
        receipt
            .events()
            .into_iter()
            .filter(|event| event.stream_id() == &secondary)
            .collect::<Vec<_>>()
    );
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

async fn seed_history(
    store: &NatsEventStore,
    aggregate: &StreamId,
) -> TestResult<Vec<RecordedEvent>> {
    let mut expected = Vec::new();
    for index in 0..7_u64 {
        let operation = format!("history-{index}");
        let outcome = store
            .append(
                aggregate,
                if index == 0 {
                    ExpectedVersion::NoStream
                } else {
                    ExpectedVersion::Exact(StreamVersion::new(u64::try_from(expected.len())?))
                },
                repeated_batch(aggregate, &operation, &operation, 100)?,
            )
            .await?;
        expected.extend_from_slice(outcome.events());
        let unrelated = stream(&format!("unrelated-{index}"))?;
        store
            .append(
                &unrelated,
                ExpectedVersion::NoStream,
                batch(&unrelated, &operation, &operation, &[b"noise"])?,
            )
            .await?;
    }
    Ok(expected)
}

async fn consumer_count(
    context: &async_nats::jetstream::Context,
    store: &NatsEventStore,
) -> TestResult<usize> {
    let stream = context.get_stream(store.config().stream_name()).await?;
    Ok(stream.cached_info().state.consumer_count)
}

#[tokio::test]
async fn history_replay_filters_interleaved_subjects_and_batches_requests() -> TestResult<()> {
    let (context, store) = directory_fixture("history-batches").await?;
    let aggregate = stream("batched")?;
    let expected = seed_history(&store, &aggregate).await?;
    // Bookkeeping is deliberately not a valid domain event and must not be delivered.
    context
        .publish(
            store.config().transaction_guard_subject("noise", 0),
            br"{}".to_vec().into(),
        )
        .await?
        .await?;
    let client = context.client();
    client.flush().await?;
    let before = client.statistics().out_messages.load(Ordering::Relaxed);
    let actual = store.load(&aggregate).await?;
    client.flush().await?;
    let requests = client
        .statistics()
        .out_messages
        .load(Ordering::Relaxed)
        .checked_sub(before)
        .ok_or("request count regressed")?;
    assert_eq!(actual, expected);
    assert!(requests <= 10, "700 events used {requests} requests");
    assert_eq!(consumer_count(&context, &store).await?, 0);
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release-mode diagnostic benchmark; run explicitly against a disposable broker"]
async fn compare_serial_raw_and_batched_history_replay() -> TestResult<()> {
    tokio::spawn(compare_history_windows()).await?
}

async fn compare_history_windows() -> TestResult<()> {
    let (context, batched) = directory_fixture("history-window-comparison").await?;
    let serial = batched.clone().with_serial_history_reads();
    let client = context.client();
    for (label, size, transactions) in [
        ("direct-100", 100_u64, false),
        ("direct-1000", 1000, false),
        ("commands-100", 100, true),
    ] {
        let writer = stream(label)?;
        let mut version = 0_u64;
        while version < size {
            let operation = format!("{label}-{version}");
            let count = if transactions { 1 } else { 100 };
            let expected = if version == 0 {
                ExpectedVersion::NoStream
            } else {
                ExpectedVersion::Exact(StreamVersion::new(version))
            };
            let events = repeated_batch(&writer, &operation, &operation, count)?;
            if transactions {
                batched
                    .append_transaction(EventTransaction::new(
                        OperationId::new(&operation)?,
                        ContentFingerprint::digest(&operation),
                        vec![TransactionParticipant::new(
                            writer.clone(),
                            expected,
                            Some(events),
                        )],
                    ))
                    .await?;
            } else {
                batched.append(&writer, expected, events).await?;
            }
            version = version
                .checked_add(u64::from(count))
                .ok_or("seed version overflow")?;
        }
        let expected = serial.load(&writer).await?;
        assert_eq!(batched.load(&writer).await?, expected);
        // Alternating ABBA blocks share the exact stream, connection, process,
        // runtime placement, and warmup. Timing thresholds are intentionally absent.
        for block in 0..3 {
            for mode in ["serial", "batched", "batched", "serial"] {
                let store = if mode == "serial" { &serial } else { &batched };
                client.flush().await?;
                let before = client.statistics().out_messages.load(Ordering::Relaxed);
                let started = std::time::Instant::now();
                let loaded = store.load(&writer).await?;
                let elapsed = started.elapsed();
                assert_eq!(loaded, expected);
                client.flush().await?;
                let requests = client
                    .statistics()
                    .out_messages
                    .load(Ordering::Relaxed)
                    .checked_sub(before)
                    .ok_or("request counter regressed")?;
                eprintln!(
                    "HISTORY_BENCH case={label} block={block} mode={mode} requests={requests} elapsed_us={}",
                    elapsed.as_micros()
                );
            }
        }
    }
    context
        .delete_stream(batched.config().stream_name())
        .await?;
    Ok(())
}

#[tokio::test]
async fn history_replay_crosses_byte_limited_pages() -> TestResult<()> {
    let (context, store) = directory_fixture("history-byte-pages").await?;
    let aggregate = stream("large-events")?;
    let payload = vec![42_u8; 80_000];
    let payloads = [payload.as_slice(); 9];
    let mut expected = Vec::new();
    for index in 0..10 {
        let operation = format!("large-{index}");
        let outcome = store
            .append(
                &aggregate,
                if index == 0 {
                    ExpectedVersion::NoStream
                } else {
                    ExpectedVersion::Exact(StreamVersion::new(u64::try_from(expected.len())?))
                },
                batch(&aggregate, &operation, &operation, &payloads)?,
            )
            .await?;
        expected.extend_from_slice(outcome.events());
    }
    assert_eq!(store.load(&aggregate).await?, expected);
    assert_eq!(consumer_count(&context, &store).await?, 0);
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
async fn history_replay_ignores_concurrent_appends_after_its_cutoff() -> TestResult<()> {
    let (context, store) = directory_fixture("history-cutoff").await?;
    let aggregate = stream("concurrent")?;
    let expected = seed_history(&store, &aggregate).await?;
    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let loader = store
        .clone()
        .with_history_snapshot_barriers(Arc::clone(&reached), Arc::clone(&release));
    let reading = loader.load(&aggregate);
    let append = async {
        reached.wait().await;
        // Even malformed bytes beyond the captured cutoff must not affect this read.
        context
            .publish(
                store.config().aggregate_subject(
                    aggregate.aggregate_type().as_str(),
                    aggregate.aggregate_id().as_str(),
                ),
                b"later malformed event".to_vec().into(),
            )
            .await?
            .await?;
        release.wait().await;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    let (actual, appended) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(reading, append)
    })
    .await?;
    appended?;
    assert_eq!(actual?, expected);
    assert!(matches!(store.load(&aggregate).await,
        Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory));
    assert_eq!(consumer_count(&context, &store).await?, 0);
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
async fn history_replay_cleans_up_after_cancellation() -> TestResult<()> {
    let (context, store) = directory_fixture("history-cancel").await?;
    let aggregate = stream("cancel")?;
    store
        .append(
            &aggregate,
            ExpectedVersion::NoStream,
            batch(&aggregate, "seed", "seed", &[b"one"])?,
        )
        .await?;
    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let loader = store
        .clone()
        .with_history_snapshot_barriers(Arc::clone(&reached), release);
    let task = tokio::spawn(async move { loader.load(&aggregate).await });
    tokio::time::timeout(Duration::from_secs(5), reached.wait()).await?;
    assert_eq!(consumer_count(&context, &store).await?, 1);
    task.abort();
    assert!(task.await.is_err());
    tokio::time::timeout(Duration::from_secs(5), async {
        while consumer_count(&context, &store).await? != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
async fn history_replay_does_not_touch_durable_progress() -> TestResult<()> {
    let (context, store) = directory_fixture("history-durable").await?;
    let aggregate = stream("durable")?;
    let expected = store
        .append(
            &aggregate,
            ExpectedVersion::NoStream,
            batch(&aggregate, "seed", "seed", &[b"one", b"two"])?,
        )
        .await?;
    let broker_stream = context.get_stream(store.config().stream_name()).await?;
    let consumer = broker_stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("application-progress".to_owned()),
            filter_subject: store.config().aggregate_subject(
                aggregate.aggregate_type().as_str(),
                aggregate.aggregate_id().as_str(),
            ),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            ..Default::default()
        })
        .await?;
    let before = consumer.get_info().await?;
    assert_eq!(store.load(&aggregate).await?, expected.events());
    let after = consumer.get_info().await?;
    assert_eq!(before.delivered, after.delivered);
    assert_eq!(before.ack_floor, after.ack_floor);
    assert_eq!(before.num_pending, after.num_pending);
    assert_eq!(consumer_count(&context, &store).await?, 1);
    assert!(store.load(&stream("absent")?).await?.is_empty());
    assert_eq!(consumer_count(&context, &store).await?, 1);
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}
