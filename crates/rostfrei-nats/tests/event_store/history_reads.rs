#![allow(clippy::panic_in_result_fn)]

use super::*;

#[tokio::test]
async fn transaction_history_reuses_stream_metadata_and_loaded_first_events() -> TestResult<()> {
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
        // One event read and two receipt-layout lookups per transaction, with a
        // small fixed allowance for metadata and range boundaries. Stream-info
        // requests and first-event rereads must not recur per transaction.
        let budget = size
            .checked_mul(3)
            .and_then(|count| count.checked_add(16))
            .ok_or("request budget overflowed")?;
        assert!(
            requests <= budget,
            "history of {size} transactions used {requests} requests; budget {budget}"
        );
        eprintln!("history of {size} transactions: {requests} requests");
    }
    context.delete_stream(store.config().stream_name()).await?;
    Ok(())
}

#[tokio::test]
async fn parallel_history_reads_preserve_commits_across_sparse_ranges() -> TestResult<()> {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release-mode diagnostic benchmark; run explicitly against a disposable broker"]
async fn compare_serial_and_parallel_history_windows() -> TestResult<()> {
    tokio::spawn(compare_history_windows()).await?
}

async fn compare_history_windows() -> TestResult<()> {
    let (context, parallel) = directory_fixture("history-window-comparison").await?;
    let serial = parallel.clone().with_serial_history_reads();
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
                parallel
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
                parallel.append(&writer, expected, events).await?;
            }
            version = version
                .checked_add(u64::from(count))
                .ok_or("seed version overflow")?;
        }
        let expected = serial.load(&writer).await?;
        assert_eq!(parallel.load(&writer).await?, expected);
        // Alternating ABBA blocks share the exact stream, connection, process,
        // runtime placement, and warmup. Timing thresholds are intentionally absent.
        for block in 0..3 {
            for mode in ["serial", "parallel", "parallel", "serial"] {
                let store = if mode == "serial" { &serial } else { &parallel };
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
        .delete_stream(parallel.config().stream_name())
        .await?;
    Ok(())
}
