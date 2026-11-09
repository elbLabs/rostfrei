#![allow(clippy::panic_in_result_fn)]

#[allow(dead_code)]
mod correlation_history_support;

use std::{collections::BTreeSet, time::Duration};

use async_nats::{
    HeaderMap,
    jetstream::{
        consumer::{self, AckPolicy, DeliverPolicy},
        stream::RetentionPolicy,
    },
};
use futures_util::StreamExt;
use rostfrei_core::{
    ContentFingerprint, CorrelationHistory, CorrelationRef, DomainEventRef, EventStore,
    EventStoreErrorKind, EventTransaction, OperationId, RecordedEvent,
};
use rostfrei_messaging_core::{CorrelationId, TrafficScope};
use rostfrei_nats::{CORRELATION_ID_HEADER, NatsCorrelationHistory};
use serde_json::Value;

use correlation_history_support::{
    Fixture, READ_TIMEOUT, TestResult, append_domain, chain, integration_envelope, new_connection,
    stream_names, unique_application, wait_for_consumer_count,
};

#[tokio::test]
async fn native_union_accepts_the_full_256_correlation_anchor_window() -> TestResult {
    let fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "large-anchor", "anchor-255", "command", 1).await?;
    append_domain(
        &fixture.sales,
        "unselected-tail",
        "outside-window",
        "command",
        1,
    )
    .await?;
    let reader = fixture.reader().await?;
    let mut ids = (0..256)
        .map(|index| CorrelationId::new(format!("anchor-{index}")))
        .collect::<Result<Vec<_>, _>>()?;
    let result = reader.load_correlations(&ids).await;
    assert!(
        result.is_ok(),
        "the Tracer anchor window must fit one native read: {result:?}"
    );
    let read = result?;
    assert_eq!(read.chains.len(), 256);
    assert_eq!(chain(&read, "anchor-255")?.refs.len(), 1);
    assert_eq!(read.snapshot.sources.len(), 3);
    assert!(read.chains.iter().all(|chain| !chain.truncated));
    ids.push(CorrelationId::new("outside-anchor-window")?);
    assert!(reader.load_correlations(&ids).await.is_err());
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn native_union_reads_two_contexts_and_one_shared_integration_snapshot() -> TestResult {
    let fixture = Fixture::in_scope(unique_application()?, TrafficScope::Test).await?;
    let escaped = "flow.*>%/~";
    let sales = append_domain(&fixture.sales, "order/one", escaped, "command.one", 2).await?;
    let shipping =
        append_domain(fixture.shipping()?, "shipment", "second", "command.two", 1).await?;
    let shared = append_domain(fixture.shipping()?, "shared", escaped, "command.one", 1).await?;
    let public_sales = fixture
        .publish("sales", "public-sales", escaped, Some("command.one"))
        .await?;
    let public_shipping = fixture
        .publish("shipping", "public-shipping", "second", None)
        .await?;
    append_domain(&fixture.sales, "noise", "flow.x", "command.one", 1).await?;
    fixture
        .publish("shipping", "noise-public", "flow.x", None)
        .await?;
    // The selected zero-event receipt is shipping's cutoff witness. Its subject
    // is already covered by the union; adding an exact filter would overlap it.
    fixture
        .shipping()?
        .append_transaction(
            EventTransaction::new(
                OperationId::new("filter-witness")?,
                ContentFingerprint::digest("filter-witness"),
                Vec::new(),
            )
            .with_bounded_context(fixture.shipping()?.config().bounded_context().clone())
            .with_correlation_id(CorrelationId::new("decisions-only")?),
        )
        .await?;
    let expected_snapshot = fixture.snapshot().await?;

    let mut creates = fixture
        .connection
        .client()
        .subscribe("$JS.API.CONSUMER.CREATE.>")
        .await?;
    fixture.connection.client().flush().await?;
    let reader = fixture.reader().await?;
    let ids = [
        CorrelationId::new(escaped)?,
        CorrelationId::new("second")?,
        CorrelationId::new("decisions-only")?,
    ];
    let result = tokio::time::timeout(READ_TIMEOUT, reader.load_correlations(&ids)).await?;
    assert!(
        result.is_ok(),
        "retained domain and integration history must be readable: {result:?}"
    );
    let mut read = result?;
    assert_eq!(read.chains.len(), 3);
    let first = chain(&read, escaped)?;
    assert!(!first.truncated);
    assert_eq!(first.refs.len(), 4);
    assert_domain_refs(&first.refs, "sales", &sales)?;
    assert_domain_refs(&first.refs, "shipping", &shared)?;
    assert!(
        first
            .refs
            .contains(&CorrelationRef::Integration(public_sales))
    );
    let second = chain(&read, "second")?;
    assert!(!second.truncated);
    assert_eq!(second.refs.len(), 2);
    assert_domain_refs(&second.refs, "shipping", &shipping)?;
    assert!(
        second
            .refs
            .contains(&CorrelationRef::Integration(public_shipping))
    );
    let empty = chain(&read, "decisions-only")?;
    assert!(empty.refs.is_empty());
    assert!(!empty.truncated);
    for chain in &read.chains {
        assert!(chain.refs.windows(2).all(|pair| match pair {
            [left, right] =>
                (left.occurred_at(), left.identity()) <= (right.occurred_at(), right.identity()),
            _ => true,
        }), "cross-source refs must have stable timestamp/identity ordering: {chain:?}");
    }
    read.snapshot
        .sources
        .sort_by(|left, right| left.stream.cmp(&right.stream));
    assert_eq!(read.snapshot.sources, expected_snapshot);

    let mut native_filters = Vec::new();
    for _ in 0..3 {
        let message = tokio::time::timeout(Duration::from_secs(2), creates.next())
            .await?
            .ok_or("native replay request subscription ended")?;
        let request: Value = serde_json::from_slice(&message.payload)?;
        assert_eq!(request["config"]["ack_policy"], "none");
        assert_eq!(request["config"]["deliver_policy"], "all");
        assert!(
            request
                .pointer("/config/durable_name")
                .is_none_or(Value::is_null)
        );
        let stream = request
            .get("stream_name")
            .and_then(Value::as_str)
            .ok_or("replay stream")?;
        let filters = request
            .pointer("/config/filter_subjects")
            .and_then(Value::as_array)
            .ok_or("native multi-correlation filter union")?;
        let filters = filters
            .iter()
            .map(|filter| {
                filter
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("non-string native filter")
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        native_filters.push((stream.to_owned(), filters));
    }
    native_filters.sort();
    let mut expected_filters = Vec::new();
    for config in fixture.configs() {
        let prefix = config.subject_prefix();
        let mut filters = BTreeSet::from([
            format!("{prefix}.transaction.*.receipt.flow%2E%2A%3E%25%2F%7E.*"),
            format!("{prefix}.fixture.*.receipt.flow%2E%2A%3E%25%2F%7E.*"),
            format!("{prefix}.transaction.*.receipt.second.*"),
            format!("{prefix}.fixture.*.receipt.second.*"),
            format!("{prefix}.transaction.*.receipt.decisions-only.*"),
            format!("{prefix}.fixture.*.receipt.decisions-only.*"),
        ]);
        if config.stream_name() == fixture.sales.config().stream_name() {
            let journal = fixture.js().get_stream(config.stream_name()).await?;
            filters.insert(
                journal
                    .get_raw_message(journal.cached_info().state.last_sequence)
                    .await?
                    .subject
                    .to_string(),
            );
        }
        expected_filters.push((config.stream_name().to_owned(), filters));
    }
    let application = fixture.topology.application().as_str();
    let integration = fixture
        .js()
        .get_stream(fixture.topology.integration_event_stream().as_str())
        .await?;
    let witness = integration
        .get_raw_message(integration.cached_info().state.last_sequence)
        .await?;
    expected_filters.push((
        fixture
            .topology
            .integration_event_stream()
            .as_str()
            .to_owned(),
        BTreeSet::from([
            format!("{application}.test.integration.*.*.flow%2E%2A%3E%25%2F%7E.*"),
            format!("{application}.test.integration.*.*.second.*"),
            format!("{application}.test.integration.*.*.decisions-only.*"),
            witness.subject.to_string(),
        ]),
    ));
    expected_filters.sort();
    assert_eq!(native_filters, expected_filters);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), creates.next())
            .await
            .is_err(),
        "a source was replayed more than once"
    );
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn repeated_subjects_cross_native_batches_and_sparse_or_empty_matches_terminate() -> TestResult
{
    let fixture = Fixture::new().await?;
    // Integration discovery crosses the native 500-message batch here. Domain
    // discovery paginates receipts, covered by the 501-receipt contract.
    let events = append_domain(&fixture.sales, "many", "selected", "same-cause", 2).await?;
    for index in 0..501 {
        fixture
            .publish(
                "sales",
                &format!("integration-{index}"),
                "selected",
                Some("same-cause"),
            )
            .await?;
    }
    // Every physical cutoff is outside the requested filter, including a source
    // with no matches. Waiting for a filtered message at the global cutoff hangs.
    append_domain(&fixture.sales, "tail", "noise", "noise", 1).await?;
    append_domain(fixture.shipping()?, "only-noise", "noise", "noise", 1).await?;
    fixture
        .publish("shipping", "tail-public", "noise", None)
        .await?;
    let reader = fixture.reader().await?;
    let ids = [
        CorrelationId::new("selected")?,
        CorrelationId::new("absent")?,
    ];
    let read = tokio::time::timeout(READ_TIMEOUT, reader.load_correlations(&ids)).await??;
    let selected = chain(&read, "selected")?;
    assert!(!selected.truncated);
    assert_eq!(selected.refs.len(), 503);
    assert_domain_refs(&selected.refs, "sales", &events)?;
    let public_ids: BTreeSet<_> = selected
        .refs
        .iter()
        .filter_map(|reference| match reference {
            CorrelationRef::Integration(reference) => Some(reference.message_id.clone()),
            CorrelationRef::Domain(_) => None,
        })
        .collect();
    assert_eq!(
        public_ids,
        (0..501)
            .map(|index| format!("integration-{index}"))
            .collect()
    );
    let absent = chain(&read, "absent")?;
    assert!(absent.refs.is_empty());
    assert!(!absent.truncated);
    let empty = tokio::time::timeout(
        Duration::from_secs(3),
        reader.load_correlations(&[CorrelationId::new("never-stored")?]),
    )
    .await??;
    assert!(chain(&empty, "never-stored")?.refs.is_empty());
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn retained_history_survives_reader_and_connection_recreation_without_a_projector()
-> TestResult {
    let mut fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "before-reader", "flow", "root", 1).await?;
    fixture
        .publish("shipping", "before-reader-public", "flow", Some("root"))
        .await?;
    let original_streams = stream_names(fixture.js()).await?;
    let reader = fixture.reader().await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    wait_for_consumer_count(&fixture, 0).await?;
    let ids = [CorrelationId::new("flow")?];
    let first = reader.load_correlations(&ids).await?;
    assert_eq!(chain(&first, "flow")?.refs.len(), 2);
    drop(reader);
    append_domain(fixture.shipping()?, "between-readers", "flow", "root", 1).await?;
    fixture.connection.drain().await?;
    fixture.connection = new_connection().await?;
    let reader =
        NatsCorrelationHistory::connect(fixture.js().clone(), fixture.configs(), &fixture.topology)
            .await?;
    wait_for_consumer_count(&fixture, 0).await?;
    let replayed = tokio::time::timeout(READ_TIMEOUT, reader.load_correlations(&ids)).await??;
    let replayed_chain = chain(&replayed, "flow")?;
    assert_eq!(replayed_chain.refs.len(), 3);
    assert!(!replayed_chain.truncated);
    for reference in &chain(&first, "flow")?.refs {
        assert!(replayed_chain.refs.contains(reference));
    }
    assert_eq!(
        stream_names(fixture.js()).await?,
        original_streams,
        "on-demand history must not create a KV index or projector stream"
    );
    wait_for_consumer_count(&fixture, 0).await?;
    drop(reader);
    fixture.cleanup().await
}

#[tokio::test]
async fn expired_integration_evidence_disappears_while_domain_facts_remain() -> TestResult {
    let fixture = Fixture::new().await?;
    let events = append_domain(&fixture.sales, "retained", "flow", "root", 1).await?;
    fixture
        .publish("sales", "expiring-public", "flow", Some("root"))
        .await?;
    let reader = fixture.reader().await?;
    let ids = [CorrelationId::new("flow")?];
    assert_eq!(
        chain(&reader.load_correlations(&ids).await?, "flow")?
            .refs
            .len(),
        2
    );
    let mut integration = fixture
        .js()
        .get_stream(fixture.topology.integration_event_stream().as_str())
        .await?;
    let mut config = integration.cached_info().config.clone();
    assert_eq!(config.max_age, Duration::from_hours(720));
    config.max_age = Duration::from_millis(250);
    config.duplicate_window = Duration::from_millis(100);
    fixture.js().update_stream(config).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if integration.info().await?.state.messages == 0 {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    let expired = tokio::time::timeout(READ_TIMEOUT, reader.load_correlations(&ids)).await??;
    let retained = chain(&expired, "flow")?;
    assert_eq!(
        retained.refs.len(),
        1,
        "expired integration metadata must not be fabricated"
    );
    assert!(!retained.truncated);
    assert_domain_refs(&retained.refs, "sales", &events)?;
    assert_eq!(
        expired.snapshot.sources.len(),
        3,
        "an empty retained source still has a cutoff"
    );
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn a_missing_domain_or_integration_source_is_an_error_not_an_empty_chain() -> TestResult {
    for remove_domain in [true, false] {
        let fixture = Fixture::new().await?;
        let reader = fixture.reader().await?;
        let missing = if remove_domain {
            fixture.shipping()?.config().stream_name()
        } else {
            fixture.topology.integration_event_stream().as_str()
        };
        fixture.js().delete_stream(missing).await?;
        let result = tokio::time::timeout(
            READ_TIMEOUT,
            reader.load_correlations(&[CorrelationId::new("never-stored")?]),
        )
        .await?;
        assert!(
            matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::Unavailable),
            "missing source {missing} was treated as complete evidence: {result:?}"
        );
        fixture.cleanup().await?;
    }
    Ok(())
}

#[tokio::test]
async fn selected_integration_records_require_envelope_subject_and_header_agreement() -> TestResult
{
    let fixture = Fixture::new().await?;
    let reader = fixture.reader().await?;
    let address = fixture
        .topology
        .application()
        .bounded_context("sales")?
        .integration_event_address("order-updated")?;
    for fault in [
        "payload",
        "subject-correlation",
        "subject-causation",
        "header-correlation",
        "header-id",
        "invalid-token",
    ] {
        let correlation = format!("selected-{fault}");
        let envelope_correlation = if fault == "subject-correlation" {
            "another-flow"
        } else {
            &correlation
        };
        let envelope = integration_envelope(fault, envelope_correlation, Some("root"))?;
        let payload = if fault == "payload" {
            b"not-an-envelope".to_vec()
        } else {
            serde_json::to_vec(&envelope)?
        };
        let cause_token = match fault {
            "subject-causation" => "another-cause",
            "invalid-token" => "bad%ZZ",
            _ => "root",
        };
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert(
            "Nats-Msg-Id",
            if fault == "header-id" {
                "wrong-id"
            } else {
                fault
            },
        );
        headers.insert(
            CORRELATION_ID_HEADER,
            if fault == "header-correlation" {
                "wrong-flow"
            } else {
                &correlation
            },
        );
        fixture
            .js()
            .publish_with_headers(
                format!("{}.{correlation}.{cause_token}", address.as_str()),
                headers,
                payload.into(),
            )
            .await?
            .await?;
        let result = tokio::time::timeout(
            READ_TIMEOUT,
            reader.load_correlations(&[CorrelationId::new(&correlation)?]),
        )
        .await?;
        assert!(
            matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory),
            "selected corrupt {fault} record was silently omitted or trusted: {result:?}"
        );
    }
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn a_domain_envelope_on_another_aggregate_subject_is_not_trusted() -> TestResult {
    let fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "real-aggregate", "flow", "root", 1).await?;
    let stream = fixture
        .js()
        .get_stream(fixture.sales.config().stream_name())
        .await?;
    let original = stream.get_raw_message(1).await?;
    let receipt = stream.get_raw_message(2).await?;
    // Reconstruct a corrupted journal before connecting the reader: the original
    // checked receipt still selects the event, but its aggregate subject lies.
    fixture
        .js()
        .delete_stream(fixture.sales.config().stream_name())
        .await?;
    fixture
        .js()
        .create_stream(fixture.sales.config().stream_config())
        .await?;
    for (subject, record) in [
        (
            fixture
                .sales
                .config()
                .event_subject("Order", "another-aggregate"),
            original,
        ),
        (receipt.subject.to_string(), receipt),
    ] {
        let response = fixture
            .connection
            .client()
            .send_request(
                subject,
                async_nats::Request::new()
                    .headers(record.headers)
                    .payload(record.payload)
                    .timeout(Some(READ_TIMEOUT)),
            )
            .await?;
        if !response.payload.is_empty() {
            let ack: Value = serde_json::from_slice(&response.payload)?;
            assert!(
                ack.get("error").is_none(),
                "corrupt-journal setup was refused: {ack}"
            );
        }
    }
    let reader = fixture.reader().await?;
    let result = tokio::time::timeout(
        READ_TIMEOUT,
        reader.load_correlations(&[CorrelationId::new("flow")?]),
    )
    .await?;
    assert!(
        matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory),
        "transport/envelope mismatch was trusted: {result:?}"
    );
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn normal_and_test_history_are_separate_and_application_durables_do_not_advance() -> TestResult
{
    let normal = Fixture::new().await?;
    let test = Fixture::in_scope(normal.topology.application().clone(), TrafficScope::Test).await?;
    for (fixture, label) in [(&normal, "normal"), (&test, "test")] {
        append_domain(&fixture.sales, label, "same-flow", label, 1).await?;
        append_domain(fixture.shipping()?, label, "same-flow", label, 1).await?;
        fixture
            .publish("sales", label, "same-flow", Some(label))
            .await?;
    }
    for (fixture, label) in [(&normal, "normal"), (&test, "test")] {
        let mut workers = Vec::new();
        for source in fixture.source_names() {
            let stream = fixture.js().get_stream(source).await?;
            let filter_subject = fixture
                .configs()
                .iter()
                .find(|config| config.stream_name() == source)
                .map_or_else(
                    String::new,
                    rostfrei_nats::NatsEventStoreConfig::aggregate_subject_filter,
                );
            let worker: consumer::PullConsumer = stream
                .create_consumer(consumer::pull::Config {
                    durable_name: Some("application-worker".to_owned()),
                    name: Some("application-worker".to_owned()),
                    deliver_policy: DeliverPolicy::All,
                    ack_policy: AckPolicy::Explicit,
                    filter_subject,
                    ..Default::default()
                })
                .await?;
            workers.push(worker);
        }
        let reader = fixture.reader().await?;
        let read = tokio::time::timeout(
            READ_TIMEOUT,
            reader.load_correlations(&[CorrelationId::new("same-flow")?]),
        )
        .await??;
        let selected = chain(&read, "same-flow")?;
        assert_eq!(selected.refs.len(), 3);
        assert!(!selected.truncated);
        for reference in &selected.refs {
            let cause = match reference {
                CorrelationRef::Domain(reference) => reference.causation_id.as_deref(),
                CorrelationRef::Integration(reference) => reference.causation_id.as_deref(),
            };
            assert_eq!(
                cause,
                Some(label),
                "another traffic scope leaked into history"
            );
        }
        for mut worker in workers {
            let info = worker.info().await?;
            assert_eq!(info.num_pending, 1);
            assert_eq!(info.num_ack_pending, 0);
            assert_eq!(info.delivered.stream_sequence, 0);
            assert_eq!(info.ack_floor.stream_sequence, 0);
        }
        wait_for_consumer_count(fixture, 1).await?;
    }
    normal.cleanup().await?;
    test.cleanup().await
}

#[tokio::test]
async fn reference_and_byte_bounds_explicitly_truncate_including_unresolved_empty_chains()
-> TestResult {
    let fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "bounded", "selected", "root", 3).await?;
    fixture
        .publish("shipping", "bounded-public", "selected", None)
        .await?;
    let ids = [
        CorrelationId::new("selected")?,
        CorrelationId::new("absent")?,
    ];
    let limited = fixture.reader().await?.with_limits(2, 32 * 1024 * 1024)?;
    let read = tokio::time::timeout(READ_TIMEOUT, limited.load_correlations(&ids)).await??;
    assert_eq!(chain(&read, "selected")?.refs.len(), 2);
    assert!(
        read.chains.iter().all(|chain| chain.truncated),
        "a bounded union must not describe an unresolved empty chain as complete"
    );
    assert_eq!(read.snapshot.sources.len(), 3);
    let tiny = fixture.reader().await?.with_limits(20, 1)?;
    let read = tokio::time::timeout(READ_TIMEOUT, tiny.load_correlations(&ids)).await??;
    assert!(
        read.chains
            .iter()
            .all(|chain| chain.truncated && chain.refs.is_empty())
    );
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn cancellation_during_native_consumer_creation_reclaims_the_created_consumer() -> TestResult
{
    let fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "cancelled", "selected", "root", 501).await?;
    let mut requests = fixture
        .connection
        .client()
        .subscribe("$JS.API.CONSUMER.CREATE.>")
        .await?;
    let mut created = fixture
        .connection
        .client()
        .subscribe("$JS.EVENT.ADVISORY.CONSUMER.CREATED.>")
        .await?;
    fixture.connection.client().flush().await?;
    let reader = fixture.reader().await?;
    let task = tokio::spawn(async move {
        reader
            .load_correlations(&[CorrelationId::new("selected")?])
            .await
            .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })
    });
    let request = tokio::time::timeout(READ_TIMEOUT, requests.next())
        .await?
        .ok_or("native consumer was never requested")?;
    let request: Value = serde_json::from_slice(&request.payload)?;
    let name = request
        .pointer("/config/name")
        .and_then(Value::as_str)
        .ok_or("consumer name")?;
    // Abort as the create request is observed, before waiting for its response.
    // Deleting before that creation finishes can otherwise orphan a consumer.
    task.abort();
    assert!(
        task.await.is_err_and(|error| error.is_cancelled()),
        "the read completed before cancellation was exercised"
    );
    tokio::time::timeout(READ_TIMEOUT, async {
        while let Some(message) = created.next().await {
            let advisory: Value = serde_json::from_slice(&message.payload)?;
            if advisory.get("consumer").and_then(Value::as_str) == Some(name) {
                return TestResult::Ok(());
            }
        }
        Err("consumer creation advisory subscription ended".into())
    })
    .await??;
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn authoritative_domain_sources_reject_per_message_expiry() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut config = fixture
        .js()
        .get_stream(fixture.sales.config().stream_name())
        .await?
        .cached_info()
        .config
        .clone();
    config.allow_message_ttl = true;
    fixture.js().update_stream(config).await?;
    let result =
        NatsCorrelationHistory::connect(fixture.js().clone(), fixture.configs(), &fixture.topology)
            .await;
    assert!(
        matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch),
        "domain history permitting per-message expiry must be rejected"
    );
    fixture.cleanup().await
}

#[tokio::test]
async fn work_queue_and_interest_sources_are_rejected_before_reading() -> TestResult {
    for domain in [false, true] {
        for retention in [RetentionPolicy::WorkQueue, RetentionPolicy::Interest] {
            let fixture = Fixture::new().await?;
            let name = if domain {
                fixture.sales.config().stream_name()
            } else {
                fixture.topology.integration_event_stream().as_str()
            };
            let mut config = fixture
                .js()
                .get_stream(name)
                .await?
                .cached_info()
                .config
                .clone();
            fixture.js().delete_stream(name).await?;
            config.retention = retention;
            config.allow_atomic_publish = false;
            fixture.js().create_stream(config).await?;
            let result = NatsCorrelationHistory::connect(
                fixture.js().clone(),
                fixture.configs(),
                &fixture.topology,
            )
            .await;
            assert!(
                matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch),
                "{retention:?} source was accepted as read-only retained history"
            );
            wait_for_consumer_count(&fixture, 0).await?;
            fixture.cleanup().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn empty_id_queries_still_validate_sources_and_capture_their_cutoffs() -> TestResult {
    let fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "anchor", "unselected", "root", 1).await?;
    let reader = fixture.reader().await?;
    let mut read = reader.load_correlations(&[]).await?;
    assert!(read.chains.is_empty());
    read.snapshot
        .sources
        .sort_by(|left, right| left.stream.cmp(&right.stream));
    assert_eq!(read.snapshot.sources, fixture.snapshot().await?);
    wait_for_consumer_count(&fixture, 0).await?;
    fixture
        .js()
        .delete_stream(fixture.shipping()?.config().stream_name())
        .await?;
    assert!(
        reader.load_correlations(&[]).await.is_err(),
        "empty selection concealed source failure"
    );
    fixture.cleanup().await
}

#[tokio::test]
async fn messages_published_after_capture_are_excluded_until_the_next_read() -> TestResult {
    let fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "before", "flow", "root", 1).await?;
    fixture
        .publish("sales", "public-before", "flow", None)
        .await?;
    let expected_snapshot = fixture.snapshot().await?;
    let reader = fixture.reader().await?;
    let mut creates = fixture
        .connection
        .client()
        .subscribe("$JS.API.CONSUMER.CREATE.>")
        .await?;
    fixture.connection.client().flush().await?;
    let ids = [CorrelationId::new("flow")?];
    let read = reader.load_correlations(&ids);
    tokio::pin!(read);
    tokio::time::timeout(READ_TIMEOUT, async {
        tokio::select! {
            request = creates.next() => { request.ok_or("no native replay creation")?; TestResult::Ok(()) },
            result = &mut read => Err(format!("read finished before capture barrier: {result:?}").into()),
        }
    }).await??;
    // The read future is deliberately not polled while later records are stored.
    append_domain(fixture.shipping()?, "after", "flow", "root", 1).await?;
    fixture
        .publish("shipping", "public-after", "flow", None)
        .await?;
    let mut first = tokio::time::timeout(READ_TIMEOUT, &mut read).await??;
    assert_eq!(chain(&first, "flow")?.refs.len(), 2);
    first
        .snapshot
        .sources
        .sort_by(|left, right| left.stream.cmp(&right.stream));
    assert_eq!(first.snapshot.sources, expected_snapshot);
    let second = reader.load_correlations(&ids).await?;
    assert_eq!(chain(&second, "flow")?.refs.len(), 4);
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn witness_subject_overfetch_consumes_budget_for_an_empty_selection() -> TestResult {
    let fixture = Fixture::new().await?;
    for index in 0..32 {
        fixture
            .publish(
                "sales",
                &format!("witness-overfetch-{index}"),
                "unselected",
                None,
            )
            .await?;
    }
    let ids = [CorrelationId::new("absent")?];
    let reader = fixture.reader().await?;
    let complete = reader.load_correlations(&ids).await?;
    assert!(chain(&complete, "absent")?.refs.is_empty());
    assert!(!chain(&complete, "absent")?.truncated);
    // The shared integration subject traverses earlier unrequested records too.
    // Both the raw witness and all delivered overfetch are examined data.
    for bytes in [1, 4096] {
        let limited = reader
            .clone()
            .with_limits(2048, bytes)?
            .load_correlations(&ids)
            .await?;
        assert!(chain(&limited, "absent")?.refs.is_empty());
        assert!(
            chain(&limited, "absent")?.truncated,
            "witness work escaped the {bytes}-byte budget: {limited:?}"
        );
    }
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn expired_cutoff_witness_is_unavailable_while_earlier_records_remain() -> TestResult {
    let fixture = Fixture::new().await?;
    let name = fixture.topology.integration_event_stream().as_str();
    let mut integration = fixture.js().get_stream(name).await?;
    let mut config = integration.cached_info().config.clone();
    config.allow_message_ttl = true;
    fixture.js().update_stream(config).await?;
    fixture
        .publish("sales", "retained-predecessor", "selected", None)
        .await?;
    let reader = fixture.reader().await?;
    let mut creates = fixture
        .connection
        .client()
        .subscribe(format!("$JS.API.CONSUMER.CREATE.{name}.>"))
        .await?;
    fixture.connection.client().flush().await?;
    let address = fixture
        .topology
        .application()
        .bounded_context("sales")?
        .integration_event_address("order-updated")?;
    let envelope = integration_envelope("expiring-witness", "unselected", None)?;
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json");
    headers.insert("Nats-Msg-Id", "expiring-witness");
    headers.insert(CORRELATION_ID_HEADER, "unselected");
    headers.insert("Nats-TTL", "2s");
    fixture
        .js()
        .publish_with_headers(
            format!("{}.unselected.~", address.as_str()),
            headers,
            serde_json::to_vec(&envelope)?.into(),
        )
        .await?
        .await?;
    let ids = [CorrelationId::new("selected")?];
    let read = reader.load_correlations(&ids);
    tokio::pin!(read);
    tokio::time::timeout(READ_TIMEOUT, async {
        tokio::select! {
            biased;
            request = creates.next() => { request.ok_or("no integration replay creation")?; TestResult::Ok(()) },
            result = &mut read => Err(format!("read completed before the witness expiry barrier: {result:?}").into()),
        }
    }).await??;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state = &integration.info().await?.state;
            if state.messages == 1 && state.last_sequence == 2 {
                assert_eq!(state.first_sequence, 1);
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    let result = read.await;
    assert!(
        matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::Unavailable),
        "pending-empty after witness expiry concealed a retained predecessor: {result:?}"
    );
    // A new capture cannot obtain C either. Missing C alone proves no coverage.
    let result = reader.load_correlations(&ids).await;
    assert!(
        matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::Unavailable),
        "missing raw cutoff witness was accepted despite retained predecessors: {result:?}"
    );
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn source_replacement_during_a_read_is_unavailable_not_mixed_history() -> TestResult {
    let fixture = Fixture::new().await?;
    append_domain(&fixture.sales, "old", "flow", "root", 1).await?;
    let reader = fixture.reader().await?;
    let mut creates = fixture
        .connection
        .client()
        .subscribe("$JS.API.CONSUMER.CREATE.>")
        .await?;
    fixture.connection.client().flush().await?;
    let ids = [CorrelationId::new("flow")?];
    let read = reader.load_correlations(&ids);
    tokio::pin!(read);
    tokio::time::timeout(READ_TIMEOUT, async {
        tokio::select! {
            request = creates.next() => { request.ok_or("no native replay creation")?; TestResult::Ok(()) },
            result = &mut read => Err(format!("read finished before replacement barrier: {result:?}").into()),
        }
    }).await??;
    fixture
        .js()
        .delete_stream(fixture.sales.config().stream_name())
        .await?;
    fixture
        .js()
        .create_stream(fixture.sales.config().stream_config())
        .await?;
    let replacement = rostfrei_nats::NatsEventStore::connect(
        fixture.js().clone(),
        fixture.sales.config().clone(),
    )
    .await?;
    append_domain(&replacement, "replacement", "flow", "root", 1).await?;
    let result = tokio::time::timeout(READ_TIMEOUT, &mut read).await?;
    assert!(
        matches!(result, Err(ref error) if error.kind() == EventStoreErrorKind::Unavailable),
        "source replacement yielded mixed or apparently complete evidence: {result:?}"
    );
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn expiry_after_native_pending_count_before_first_delivery_finishes_with_broker_proof()
-> TestResult {
    let fixture = Fixture::new().await?;
    let integration_name = fixture.topology.integration_event_stream().as_str();
    let mut integration = fixture.js().get_stream(integration_name).await?;
    let mut config = integration.cached_info().config.clone();
    // Allow broker setup under workspace load; the expiry clock starts only
    // after the reader and creation barrier are ready below.
    config.max_age = Duration::from_secs(8);
    config.duplicate_window = Duration::from_millis(100);
    fixture.js().update_stream(config).await?;
    append_domain(&fixture.sales, "retained", "flow", "root", 1).await?;
    let reader = fixture.reader().await?;
    let mut creates = fixture
        .connection
        .client()
        .subscribe(format!("$JS.API.CONSUMER.CREATE.{integration_name}.>"))
        .await?;
    fixture.connection.client().flush().await?;
    fixture
        .publish("sales", "expires-in-flight", "flow", None)
        .await?;
    let ids = [CorrelationId::new("flow")?];
    let read = reader.load_correlations(&ids);
    tokio::pin!(read);
    let request = tokio::time::timeout(READ_TIMEOUT, async {
        tokio::select! {
            biased;
            // Observe creation before polling the read again: otherwise a ready
            // response can start native prefetch before this expiry barrier.
            request = creates.next() => TestResult::Ok(request.ok_or("no native replay creation")?),
            result = &mut read => Err(format!("read finished before expiry barrier: {result:?}").into()),
        }
    }).await??;
    let request: Value = serde_json::from_slice(&request.payload)?;
    let name = request
        .pointer("/config/name")
        .and_then(Value::as_str)
        .ok_or("consumer name")?;
    let pending = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(info) = integration.consumer_info(name).await {
                return info.num_pending;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(
        pending, 1,
        "the consumer's initial finite selection must include the expiring record"
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if integration.info().await?.state.messages == 0 {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    let read = tokio::time::timeout(Duration::from_secs(3), &mut read).await??;
    let retained = chain(&read, "flow")?;
    assert_eq!(retained.refs.len(), 1);
    assert!(!retained.truncated);
    assert!(matches!(
        retained.refs.first(),
        Some(CorrelationRef::Domain(_))
    ));
    wait_for_consumer_count(&fixture, 0).await?;
    fixture.cleanup().await
}

fn assert_domain_refs(
    refs: &[CorrelationRef],
    context: &str,
    events: &[RecordedEvent],
) -> TestResult {
    let actual: Vec<_> = refs
        .iter()
        .filter_map(|reference| match reference {
            CorrelationRef::Domain(reference) if reference.context == context => Some(reference),
            _ => None,
        })
        .collect();
    assert_eq!(actual.len(), events.len());
    for event in events {
        let reference = actual
            .iter()
            .find(|reference| reference.event_id == event.event_id().as_str())
            .ok_or("committed domain event missing from history")?;
        assert_eq!(
            **reference,
            DomainEventRef::from_recorded(context, event, reference.occurred_at)
        );
    }
    Ok(())
}
