#![allow(clippy::panic_in_result_fn)]

#[allow(dead_code)]
mod correlation_history_support;
mod receipt_history_review_support;

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use rostfrei_core::{
    AggregateId, AggregateType, CommandCompletion, CommandExecutionMetadata, ContentFingerprint,
    CorrelationHistory, CorrelationRef, DomainEventRef, EventBatch, EventStore, EventTransaction,
    ExpectedVersion, FixtureEventStore, JsonRejectionCodec, NewEvent, OperationId, OutcomeKey,
    OutcomeLifecycle, OutcomeLookup, OutcomeMarker, OutcomeReader, RecordedEvent, RejectionCodec,
    StreamId, StreamVersion, TransactionParticipant, TransactionReceipt, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::{CausationId, CorrelationId, MessageTimestamp, TrafficScope};
use rostfrei_nats::{NatsCorrelationHistory, NatsEventStore, provision_event_store};
use serde_json::Value;

use correlation_history_support::{Fixture, READ_TIMEOUT, TestResult, chain};

#[tokio::test]
async fn zero_pending_before_cutoff_delivery_is_not_complete_empty_history() -> TestResult {
    let fixture = Fixture::with_authority(TrafficScope::Normal, false).await?;
    let aggregate = StreamId::new(AggregateType::new("Order")?, AggregateId::new("pending")?);
    append_command(&fixture.sales, &aggregate, "pending", 0, "selected", 1).await?;
    let proxy = receipt_history_review_support::PendingEmptyProxy::start(
        fixture.connection.client().clone(),
    )
    .await?;
    let reader = NatsCorrelationHistory::connect(
        proxy.context.clone(),
        fixture.configs(),
        &fixture.topology,
    )
    .await?;
    let read = reader
        .load_correlations(&[CorrelationId::new("selected")?])
        .await?;
    assert!(
        proxy.rewritten() > 1,
        "creation and progress pending-empty faults were not both exercised"
    );
    let rewritten = proxy.rewritten();
    let absent = reader
        .load_correlations(&[CorrelationId::new("absent")?])
        .await?;
    assert!(chain(&absent, "absent")?.refs.is_empty());
    assert!(!chain(&absent, "absent")?.truncated);
    assert!(
        proxy.rewritten().saturating_sub(rewritten) > 1,
        "an empty selection must still await the cutoff witness"
    );
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await?;
    let selected = chain(&read, "selected")?;
    assert_eq!(
        selected.refs.len(),
        1,
        "a pending-empty consumer omitted the retained event before proving cutoff coverage: {read:?}"
    );
    assert!(!selected.truncated);
    Ok(())
}

#[tokio::test]
async fn attached_reader_rejects_replacement_between_reads_even_with_reused_event_ids() -> TestResult
{
    let mut fixture = Fixture::with_authority(TrafficScope::Normal, false).await?;
    let aggregate = StreamId::new(AggregateType::new("Order")?, AggregateId::new("reused")?);
    let original = append_command(&fixture.sales, &aggregate, "reused", 0, "selected", 1).await?;
    let reader = fixture.reader().await?;
    let ids = [CorrelationId::new("selected")?];
    assert_eq!(
        chain(&reader.load_correlations(&ids).await?, "selected")?
            .refs
            .len(),
        1
    );
    let config = fixture.sales.config().clone();
    fixture
        .connection
        .jetstream()
        .delete_stream(config.stream_name())
        .await?;
    provision_event_store(fixture.connection.jetstream(), &config).await?;
    fixture.sales = NatsEventStore::connect(fixture.connection.jetstream().clone(), config).await?;
    let replacement =
        append_command(&fixture.sales, &aggregate, "reused", 0, "selected", 1).await?;
    assert_eq!(
        original, replacement,
        "replacement must exercise reused event identity and version"
    );
    let stale = reader.load_correlations(&ids).await;
    let fresh = fixture.reader().await?.load_correlations(&ids).await?;
    assert_eq!(chain(&fresh, "selected")?.refs.len(), 1);
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await?;
    assert!(
        stale.is_err(),
        "an attached reader adopted a replacement journal: {stale:?}"
    );
    Ok(())
}

#[tokio::test]
async fn inline_receipt_rejects_another_application_scope() -> TestResult {
    assert_foreign_receipt_scope(false, false).await
}

#[tokio::test]
async fn inline_receipt_rejects_test_scope_in_normal_history() -> TestResult {
    assert_foreign_receipt_scope(false, true).await
}

#[tokio::test]
async fn split_marker_rejects_another_application_scope() -> TestResult {
    assert_foreign_receipt_scope(true, false).await
}

#[tokio::test]
async fn split_marker_rejects_test_scope_in_normal_history() -> TestResult {
    assert_foreign_receipt_scope(true, true).await
}

#[allow(clippy::too_many_lines)]
async fn assert_foreign_receipt_scope(split: bool, test_scope: bool) -> TestResult {
    let fixture = Fixture::with_authority(TrafficScope::Normal, split).await?;
    let fingerprint = ContentFingerprint::digest("scope-review");
    let proposal = |operation: &str, scope: Option<String>| -> TestResult<EventTransaction> {
        let operation = OperationId::new(operation)?;
        let metadata = CommandExecutionMetadata::new(operation.clone(), fingerprint)
            .with_default_outcome_evidence::<()>();
        let mut evidence = metadata
            .outcome_evidence()
            .cloned()
            .ok_or("command evidence missing")?;
        evidence.authority_scope = scope;
        Ok(EventTransaction::new(operation, fingerprint, Vec::new())
            .with_bounded_context(fixture.sales.config().bounded_context().clone())
            .with_correlation_id(CorrelationId::new("selected")?)
            .with_completion(CommandCompletion::new(Some(evidence), None)))
    };
    fixture
        .sales
        .commit_outcome(proposal("seed", None)?)
        .await?;
    let wrong_scope = if test_scope {
        format!("{}:test", fixture.sales.config().application().as_str())
    } else {
        "another-application:normal".to_owned()
    };
    let invalid = proposal("foreign", Some(wrong_scope.clone()))?;
    assert!(
        fixture.sales.commit_outcome(invalid.clone()).await.is_err(),
        "write-side scope validation must reject the same evidence"
    );
    let raw = fixture.journal().await?.get_raw_message(1).await?;
    let wire: Value = serde_json::from_slice(&raw.payload)?;
    let schema = wire
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or("receipt schema missing")?;
    let text = std::str::from_utf8(&raw.payload)?;
    let content = text
        .split_once(",\"receipt\":")
        .and_then(|(_, content)| content.strip_suffix('}'))
        .ok_or("canonical receipt content missing")?;
    let mut content = content.replace("\"operationId\":\"seed\"", "\"operationId\":\"foreign\"");
    if split {
        let integrity =
            TransactionReceipt::new(invalid.operation_id().clone(), fingerprint, Vec::new())
                .with_bounded_context(fixture.sales.config().bounded_context().clone())
                .with_correlation_id(CorrelationId::new("selected")?)
                .with_completion(invalid.completion().cloned().ok_or("completion missing")?);
        let (marker, _) =
            OutcomeMarker::prepare(fixture.sales.authority().clone(), &invalid, integrity)?;
        let old = wire
            .pointer("/receipt/manifest")
            .and_then(Value::as_str)
            .ok_or("manifest missing")?;
        content = content.replace(old, &STANDARD.encode(marker.encode_manifest()?));
    } else {
        content = content.replace(
            "\"authority_scope\":null",
            &format!(
                "\"authority_scope\":{}",
                serde_json::to_string(&wrong_scope)?
            ),
        );
    }
    let checked = format!("{{\"schemaVersion\":{schema},\"receipt\":{content}}}");
    let checksum = ContentFingerprint::digest(checked.as_bytes()).to_hex();
    let payload =
        format!("{{\"schemaVersion\":{schema},\"checksum\":\"{checksum}\",\"receipt\":{content}}}");
    let mut headers = raw.headers;
    headers.insert("Nats-Batch-Id", "foreign-scope-review");
    headers.insert(
        "Nats-Expected-Last-Subject-Sequence-Subject",
        fixture
            .sales
            .config()
            .transaction_receipt_subject_filter("foreign"),
    );
    let subject =
        fixture
            .sales
            .config()
            .transaction_receipt_subject("foreign", Some("selected"), None);
    let ack = fixture
        .connection
        .client()
        .send_request(
            subject,
            async_nats::Request::new()
                .headers(headers)
                .payload(payload.into())
                .timeout(Some(READ_TIMEOUT)),
        )
        .await?;
    let ack: Value = serde_json::from_slice(&ack.payload)?;
    assert!(
        ack.get("error").is_none(),
        "corrupt-receipt setup was refused: {ack}"
    );
    let read = fixture
        .reader()
        .await?
        .load_correlations(&[CorrelationId::new("selected")?])
        .await;
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await?;
    assert!(
        read.is_err(),
        "checksummed foreign authority scope {wrong_scope} was accepted: {read:?}"
    );
    Ok(())
}

#[tokio::test]
async fn zero_event_decisions_exhaust_the_shared_examined_byte_budget() -> TestResult {
    let fixture = Fixture::with_authority(TrafficScope::Normal, false).await?;
    let journal = fixture.journal().await?;
    let mut receipt_bytes = 0_usize;
    let byte_limit = 4096;
    for index in 0_u64..32 {
        let operation = OperationId::new(format!("decision-{index}"))?;
        let fingerprint = ContentFingerprint::digest(operation.as_str());
        let metadata = CommandExecutionMetadata::new(operation.clone(), fingerprint)
            .with_default_outcome_evidence::<()>();
        let rejection = if index.is_multiple_of(2) {
            Some(JsonRejectionCodec::new("refused", 1).encode(&"no-change".to_owned())?)
        } else {
            None
        };
        let transaction = EventTransaction::new(operation, fingerprint, Vec::new())
            .with_bounded_context(fixture.sales.config().bounded_context().clone())
            .with_correlation_id(CorrelationId::new("decisions-only")?)
            .with_causation_id(CausationId::new("root")?)
            .with_completion(CommandCompletion::new(
                metadata.outcome_evidence().cloned(),
                rejection,
            ));
        let committed = fixture.sales.commit_outcome(transaction).await?;
        assert!(committed.outcome().record().events().is_empty());
        let raw = journal
            .get_raw_message(index.checked_add(1).ok_or("receipt position overflow")?)
            .await?;
        assert!(raw.payload.len() < byte_limit);
        receipt_bytes = receipt_bytes
            .checked_add(raw.payload.len())
            .ok_or("receipt byte count overflow")?;
    }
    assert!(receipt_bytes > byte_limit);
    let ids = [
        CorrelationId::new("decisions-only")?,
        CorrelationId::new("absent")?,
    ];
    let reader = fixture.reader().await?;
    let complete = reader.load_correlations(&ids).await?;
    assert_eq!(complete.chains.len(), ids.len());
    assert!(
        complete
            .chains
            .iter()
            .all(|item| item.refs.is_empty() && !item.truncated)
    );
    let limited = reader.with_limits(2048, byte_limit)?;
    let read = tokio::time::timeout(READ_TIMEOUT, limited.load_correlations(&ids)).await??;
    assert_eq!(read.chains.len(), ids.len());
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await?;
    assert!(read.chains.iter().all(|item| item.refs.is_empty()));
    assert!(
        read.chains.iter().all(|item| item.truncated),
        "32 accepted/rejected zero-event receipts ({receipt_bytes} payload bytes) exceed the \
         {byte_limit}-byte discovery budget; unresolved empty chains must be truncated: {read:?}"
    );
    Ok(())
}

#[tokio::test]
async fn receipt_discovery_crosses_native_batches_before_a_sparse_event_and_unmatched_tail()
-> TestResult {
    let fixture = Fixture::with_authority(TrafficScope::Normal, false).await?;
    for index in 0..501 {
        let operation = OperationId::new(format!("no-events-{index}"))?;
        let fingerprint = ContentFingerprint::digest(operation.as_str());
        fixture
            .sales
            .append_transaction(
                EventTransaction::new(operation, fingerprint, Vec::new())
                    .with_bounded_context(fixture.sales.config().bounded_context().clone())
                    .with_correlation_id(CorrelationId::new("selected")?),
            )
            .await?;
    }
    let aggregate = StreamId::new(AggregateType::new("Order")?, AggregateId::new("sparse")?);
    let events = append_command(&fixture.sales, &aggregate, "sparse", 0, "selected", 1).await?;
    append_command(&fixture.sales, &aggregate, "tail", 1, "unselected", 1).await?;
    let reader = fixture.reader().await?;
    let read = tokio::time::timeout(
        READ_TIMEOUT,
        reader.load_correlations(&[
            CorrelationId::new("selected")?,
            CorrelationId::new("absent")?,
        ]),
    )
    .await??;
    assert!(read.chains.iter().all(|item| !item.truncated));
    let selected = chain(&read, "selected")?;
    assert_eq!(selected.refs.len(), 1);
    let event = events.first().ok_or("seeded event missing")?;
    assert!(
        matches!(selected.refs.first(), Some(CorrelationRef::Domain(reference))
        if reference.event_id == event.event_id().as_str() && reference.stream_version == 1)
    );
    assert!(chain(&read, "absent")?.refs.is_empty());
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn normal_command_refs_resolve_original_event_positions_and_metadata() -> TestResult {
    assert_original_event_resolution(TrafficScope::Normal).await
}

#[tokio::test]
async fn command_and_test_fixture_refs_resolve_original_event_positions_and_metadata() -> TestResult
{
    assert_original_event_resolution(TrafficScope::Test).await
}

#[tokio::test]
async fn split_history_uses_journal_authority_after_detail_expiry_and_state_replacement()
-> TestResult {
    let fixture = Fixture::with_authority(TrafficScope::Normal, true).await?;
    let aggregate = StreamId::new(AggregateType::new("Order")?, AggregateId::new("retained")?);
    append_command(&fixture.sales, &aggregate, "retained", 0, "selected", 2).await?;
    let ids = [CorrelationId::new("selected")?];
    let before = fixture.reader().await?.load_correlations(&ids).await?;
    assert_eq!(chain(&before, "selected")?.refs.len(), 2);
    let key = OutcomeKey::new(
        fixture.sales.authority().clone(),
        fixture.sales.config().bounded_context().clone(),
        OperationId::new("retained")?,
    );
    let OutcomeLookup::Completed(winner) = fixture.sales.lookup_outcome(&key).await? else {
        return Err("missing committed decision before explicit detail expiry".into());
    };
    fixture
        .sales
        .expire_details(&key, winner.marker().revision())
        .await?;
    assert_eq!(
        fixture.reader().await?.load_correlations(&ids).await?,
        before
    );
    let state_name = fixture.sales.config().outcome_state_stream_name();
    let state_config = fixture
        .connection
        .jetstream()
        .get_stream(state_name)
        .await?
        .cached_info()
        .config
        .clone();
    fixture
        .connection
        .jetstream()
        .delete_stream(state_name)
        .await?;
    assert_eq!(
        fixture.reader().await?.load_correlations(&ids).await?,
        before,
        "missing retainable state must not erase journal-local historical evidence"
    );
    fixture
        .connection
        .jetstream()
        .create_stream(state_config)
        .await?;
    assert_eq!(
        fixture.reader().await?.load_correlations(&ids).await?,
        before,
        "replacement state must not rewrite the recorded historical authority"
    );
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn missing_or_forged_journal_authority_is_not_healthy_empty_history() -> TestResult {
    let fixture = Fixture::with_authority(TrafficScope::Normal, true).await?;
    let aggregate = StreamId::new(AggregateType::new("Order")?, AggregateId::new("authority")?);
    append_command(&fixture.sales, &aggregate, "authority", 0, "selected", 1).await?;
    let reader = fixture.reader().await?;
    let journal = fixture.journal().await?;
    for binding in [None, Some(journal.cached_info().created.to_string())] {
        let mut config = journal.cached_info().config.clone();
        config.metadata.remove("rostfrei.split.state-incarnation");
        if let Some(binding) = binding {
            config
                .metadata
                .insert("rostfrei.split.state-incarnation".to_owned(), binding);
        }
        fixture.connection.jetstream().update_stream(config).await?;
        assert!(
            reader
                .load_correlations(&[CorrelationId::new("selected")?])
                .await
                .is_err(),
            "missing or forged historical authority was accepted"
        );
    }
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn rejection_guards_and_checkpoints_consume_bytes_without_becoming_event_refs() -> TestResult
{
    let fixture = Fixture::with_authority(TrafficScope::Normal, false).await?;
    let aggregate = StreamId::new(AggregateType::new("Order")?, AggregateId::new("guarded")?);
    let operation = OperationId::new("seed")?;
    let fingerprint = ContentFingerprint::digest("seed");
    let commit = derive_commit_id(&aggregate, &operation);
    let seed = EventBatch::new(
        commit.clone(),
        operation.clone(),
        fingerprint,
        vec![NewEvent::new(
            derive_event_id(&commit, 0),
            "order-changed",
            1,
            vec![b'x'; 8192],
        )?],
    )?
    .with_correlation_id(CorrelationId::new("unselected")?);
    fixture
        .sales
        .append_transaction(
            EventTransaction::new(
                operation,
                fingerprint,
                vec![TransactionParticipant::new(
                    aggregate.clone(),
                    ExpectedVersion::NoStream,
                    Some(seed),
                )],
            )
            .with_bounded_context(fixture.sales.config().bounded_context().clone())
            .with_correlation_id(CorrelationId::new("unselected")?),
        )
        .await?;
    let operation = OperationId::new("rejected")?;
    let fingerprint = ContentFingerprint::digest("rejected");
    let metadata = CommandExecutionMetadata::new(operation.clone(), fingerprint)
        .with_default_outcome_evidence::<()>();
    fixture
        .sales
        .commit_outcome(
            EventTransaction::new(
                operation,
                fingerprint,
                vec![TransactionParticipant::new(
                    aggregate,
                    ExpectedVersion::Exact(StreamVersion::new(1)),
                    None,
                )],
            )
            .with_bounded_context(fixture.sales.config().bounded_context().clone())
            .with_correlation_id(CorrelationId::new("selected")?)
            .with_completion(CommandCompletion::new(
                metadata.outcome_evidence().cloned(),
                Some(JsonRejectionCodec::new("refused", 1).encode(&"no-change".to_owned())?),
            )),
        )
        .await?;
    let ids = [
        CorrelationId::new("selected")?,
        CorrelationId::new("absent")?,
    ];
    let reader = fixture.reader().await?;
    let complete = reader.load_correlations(&ids).await?;
    assert_eq!(complete.chains.len(), 2);
    assert!(
        complete
            .chains
            .iter()
            .all(|item| item.refs.is_empty() && !item.truncated)
    );
    let limited = reader
        .with_limits(2048, 4096)?
        .load_correlations(&ids)
        .await?;
    assert_eq!(limited.chains.len(), 2);
    assert!(
        limited
            .chains
            .iter()
            .all(|item| item.refs.is_empty() && item.truncated),
        "the large unselected guard checkpoint is examined validation data: {limited:?}"
    );
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await
}

async fn assert_original_event_resolution(scope: TrafficScope) -> TestResult {
    let fixture = Fixture::with_authority(scope, false).await?;
    let aggregate = StreamId::new(AggregateType::new("Order")?, AggregateId::new("order/one")?);
    let mut selected =
        append_command(&fixture.sales, &aggregate, "first", 0, "selected", 2).await?;
    append_command(&fixture.sales, &aggregate, "noise", 2, "unselected", 1).await?;
    let mut version = 3;
    if scope == TrafficScope::Test {
        let batch = event_batch(&aggregate, "fixture", "selected", 2)?;
        selected.extend(
            fixture
                .sales
                .append_fixture(
                    &aggregate,
                    ExpectedVersion::Exact(StreamVersion::new(version)),
                    batch,
                )
                .await?
                .into_events(),
        );
        version = version.checked_add(2).ok_or("fixture version overflow")?;
    }
    selected
        .extend(append_command(&fixture.sales, &aggregate, "last", version, "selected", 1).await?);
    let journal = fixture.journal().await?;
    let cutoff = journal.cached_info().state.last_sequence;
    let mut expected = BTreeMap::new();
    // Independent setup evidence: capture the broker's original event positions
    // and timestamps before observing any inspection traffic.
    for sequence in 1..=cutoff {
        let raw = journal.get_raw_message(sequence).await?;
        let Some(event) = selected.iter().find(|event| {
            raw.headers
                .get("Nats-Msg-Id")
                .is_some_and(|id| id.as_str() == event.event_id().as_str())
        }) else {
            continue;
        };
        let milliseconds = u64::try_from(raw.time.unix_timestamp_nanos() / 1_000_000)?;
        expected.insert(
            sequence,
            CorrelationRef::Domain(DomainEventRef::from_recorded(
                "sales",
                event,
                MessageTimestamp::from_unix_milliseconds(milliseconds)?,
            )),
        );
    }
    assert_eq!(expected.len(), selected.len());
    let mut gets = fixture
        .connection
        .client()
        .subscribe(format!(
            "$JS.API.STREAM.MSG.GET.{}",
            fixture.sales.config().stream_name()
        ))
        .await?;
    fixture.connection.client().flush().await?;
    let reader = fixture.reader().await?;
    let read = tokio::time::timeout(
        READ_TIMEOUT,
        reader.load_correlations(&[CorrelationId::new("selected")?]),
    )
    .await??;
    let retained = chain(&read, "selected")?;
    assert!(!retained.truncated);
    assert_eq!(retained.refs.len(), expected.len());
    for reference in expected.values() {
        assert!(
            retained.refs.contains(reference),
            "original event metadata changed: {reference:?}; {retained:?}"
        );
    }
    let snapshot = read
        .snapshot
        .sources
        .iter()
        .find(|source| source.context.as_deref() == Some("sales"))
        .ok_or("domain cutoff missing")?;
    assert_eq!(snapshot.cutoff_sequence, cutoff.to_string());
    let mut requested_positions = BTreeSet::new();
    while let Ok(Some(message)) =
        tokio::time::timeout(Duration::from_millis(100), gets.next()).await
    {
        let request: Value = serde_json::from_slice(&message.payload)?;
        let sequence = request.get("seq").and_then(Value::as_u64).ok_or(
            "inspection used a subject/history lookup instead of an immutable event position",
        )?;
        requested_positions.insert(sequence);
    }
    fixture.assert_no_consumers().await?;
    fixture.cleanup().await?;
    assert_eq!(
        requested_positions,
        expected.keys().copied().chain([cutoff]).collect(),
        "{scope:?}: resolve the cutoff witness and exactly the receipt-selected original events through leader-served MSG.GET"
    );
    Ok(())
}

fn event_batch(
    stream: &StreamId,
    operation: &str,
    correlation: &str,
    count: u32,
) -> TestResult<EventBatch> {
    let operation = OperationId::new(operation)?;
    let commit = derive_commit_id(stream, &operation);
    let events = (0..count)
        .map(|ordinal| {
            NewEvent::new(
                derive_event_id(&commit, ordinal),
                "order-changed",
                7,
                b"original-payload".to_vec(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBatch::new(
        commit,
        operation.clone(),
        ContentFingerprint::digest(operation.as_str()),
        events,
    )?
    .with_correlation_id(CorrelationId::new(correlation)?)
    .with_causation_id(CausationId::new(format!("cause-{}", operation.as_str()))?))
}

async fn append_command(
    store: &NatsEventStore,
    stream: &StreamId,
    operation: &str,
    version: u64,
    correlation: &str,
    count: u32,
) -> TestResult<Vec<RecordedEvent>> {
    let batch = event_batch(stream, operation, correlation, count)?;
    let metadata =
        CommandExecutionMetadata::new(batch.operation_id().clone(), batch.operation_fingerprint())
            .with_default_outcome_evidence::<()>();
    let transaction = EventTransaction::new(
        batch.operation_id().clone(),
        batch.operation_fingerprint(),
        vec![TransactionParticipant::new(
            stream.clone(),
            if version == 0 {
                ExpectedVersion::NoStream
            } else {
                ExpectedVersion::Exact(StreamVersion::new(version))
            },
            Some(batch),
        )],
    )
    .with_bounded_context(store.config().bounded_context().clone())
    .with_correlation_id(CorrelationId::new(correlation)?)
    .with_causation_id(CausationId::new(format!("cause-{operation}"))?)
    .with_completion(CommandCompletion::new(
        metadata.outcome_evidence().cloned(),
        None,
    ));
    Ok(store
        .commit_outcome(transaction)
        .await?
        .outcome()
        .record()
        .events())
}
