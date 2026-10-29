#![allow(clippy::panic_in_result_fn)]

use super::*;
use async_nats::jetstream::stream::RawMessageErrorKind;
use rostfrei_messaging_core::{CausationId, CorrelationId, TrafficScope};

const EXPECTATION_SUBJECT_HEADER: &str = "Nats-Expected-Last-Subject-Sequence-Subject";

fn correlated_batch(
    aggregate: &StreamId,
    operation: &str,
    correlation: Option<&str>,
    causation: Option<&str>,
    payloads: &[&[u8]],
) -> TestResult<EventBatch> {
    let mut events = batch(aggregate, operation, operation, payloads)?;
    if let Some(correlation) = correlation {
        events = events.with_correlation_id(CorrelationId::new(correlation)?);
    }
    if let Some(causation) = causation {
        events = events.with_causation_id(CausationId::new(causation)?);
    }
    Ok(events)
}

// The broker applies next_by_subj. No envelope scanning or client-side matching
// can make these assertions pass when the subject lacks either identity.
async fn broker_subjects(
    context: &async_nats::jetstream::Context,
    store: &NatsEventStore,
    filter: &str,
) -> TestResult<Vec<String>> {
    let journal = context.get_stream(store.config().stream_name()).await?;
    let cutoff = journal.cached_info().state.last_sequence;
    let mut next = 1;
    let mut subjects = Vec::new();
    while next <= cutoff {
        match journal.get_first_raw_message_by_subject(filter, next).await {
            Ok(message) => {
                next = checked_add_u64(message.sequence, 1, "next filtered event")?;
                subjects.push(message.subject.to_string());
            }
            Err(error) if error.kind() == RawMessageErrorKind::NoMessageFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(subjects)
}

#[tokio::test]
async fn ordinary_appends_store_readable_identity_and_escaped_metadata_tokens() -> TestResult<()> {
    let (context, store) = directory_fixture("subject-tokens").await?;
    // Expected encodings are literal contract examples, independent of the
    // production token helper. An actual '~' must differ from an absent ID.
    let examples = [
        (
            "RentalFleet",
            "550e8400-e29b-41d4-a716-446655440000",
            Some("550e8400-e29b-41d4-a716-446655440001"),
            Some("550e8400-e29b-41d4-a716-446655440002"),
            "RentalFleet.550e8400-e29b-41d4-a716-446655440000.550e8400-e29b-41d4-a716-446655440001.550e8400-e29b-41d4-a716-446655440002",
        ),
        (
            "Fleet.Type-1_/*>%~é",
            "fleet.id/*>%~é",
            Some("flow.*>%/~"),
            Some("cause.*>%/~"),
            "Fleet%2EType-1_%2F%2A%3E%25%7E%C3%A9.fleet%2Eid%2F%2A%3E%25%7E%C3%A9.flow%2E%2A%3E%25%2F%7E.cause%2E%2A%3E%25%2F%7E",
        ),
        ("Fleet", "missing", None, None, "Fleet.missing.~.~"),
        (
            "Fleet",
            "literal-tilde",
            Some("~"),
            Some("~"),
            "Fleet.literal-tilde.%7E.%7E",
        ),
        (
            "Fleet",
            "missing-cause",
            Some("flow"),
            None,
            "Fleet.missing-cause.flow.~",
        ),
        (
            "Fleet",
            "missing-correlation",
            None,
            Some("cause"),
            "Fleet.missing-correlation.~.cause",
        ),
        (
            "Fleet",
            "literal-percent",
            Some("%2E"),
            Some("%7E"),
            "Fleet.literal-percent.%252E.%257E",
        ),
    ];
    let mut expected = Vec::new();
    for (index, (aggregate_type, aggregate_id, correlation, causation, suffix)) in
        examples.into_iter().enumerate()
    {
        let aggregate = StreamId::new(
            AggregateType::new(aggregate_type)?,
            AggregateId::new(aggregate_id)?,
        );
        let appended = store
            .append(
                &aggregate,
                ExpectedVersion::NoStream,
                correlated_batch(
                    &aggregate,
                    &format!("tokens-{index}"),
                    correlation,
                    causation,
                    &[b"one", b"two"],
                )?,
            )
            .await?;
        assert_eq!(store.load(&aggregate).await?, appended.events());
        for event in appended.events() {
            assert_eq!(
                event.correlation_id().map(CorrelationId::as_str),
                correlation
            );
            assert_eq!(event.causation_id().map(CausationId::as_str), causation);
            expected.push(format!(
                "{}.aggregate.{suffix}",
                store.config().subject_prefix()
            ));
        }
    }
    let actual = broker_subjects(
        &context,
        &store,
        &format!("{}.aggregate.>", store.config().subject_prefix()),
    )
    .await?;
    context.delete_stream(store.config().stream_name()).await?;
    assert_eq!(
        actual, expected,
        "stored subjects must expose four safe identity tokens"
    );
    Ok(())
}

#[tokio::test]
async fn correlated_subjects_preserve_application_scope_context_and_split_epoch() -> TestResult<()>
{
    let context = connect_context(&rostfrei_testing::integration::nats_url()?).await?;
    let (namespace, _) = unique_names("subject-scope")?;
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for scope in [TrafficScope::Normal, TrafficScope::Test] {
        let bounded_context = namespace
            .application()
            .bounded_context_in_scope(scope, "subject-scope")?;
        let config = NatsEventStoreConfig::for_bounded_context(&bounded_context)?
            .with_fresh_split_outcome_epoch("subject-format-fixture")?
            .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
        provision_event_store(&context, &config).await?;
        let store = NatsEventStore::connect(context.clone(), config.clone()).await?;
        let aggregate = stream("readable-id")?;
        store
            .append(
                &aggregate,
                ExpectedVersion::NoStream,
                correlated_batch(
                    &aggregate,
                    "scope-append",
                    Some("flow"),
                    Some("cause"),
                    &[b"event"],
                )?,
            )
            .await?;
        actual.extend(
            broker_subjects(
                &context,
                &store,
                &format!("{}.aggregate.>", config.subject_prefix()),
            )
            .await?,
        );
        expected.push(format!(
            "{}.aggregate.IntegrationAggregate.readable-id.flow.cause",
            config.subject_prefix()
        ));
        context.delete_stream(config.stream_name()).await?;
        context
            .delete_stream(config.outcome_state_stream_name())
            .await?;
    }
    assert_eq!(actual, expected);
    Ok(())
}

#[tokio::test]
async fn broker_filters_correlation_and_causation_independently_across_aggregates() -> TestResult<()>
{
    let (context, store) = directory_fixture("subject-filters").await?;
    for (aggregate_type, id, correlation, causation) in [
        ("Fleet", "first", "flow.*>%", "cause.*>%"),
        ("Reservation", "second", "flow.*>%", "other-cause"),
        ("Fleet", "third", "other-flow", "cause.*>%"),
        ("Fleet", "noise", "other-flow", "other-cause"),
        ("Fleet", "percent-noise", "flow.%2A>%", "cause.%2A>%"),
    ] {
        let aggregate = StreamId::new(AggregateType::new(aggregate_type)?, AggregateId::new(id)?);
        store
            .append(
                &aggregate,
                ExpectedVersion::NoStream,
                correlated_batch(
                    &aggregate,
                    id,
                    Some(correlation),
                    Some(causation),
                    &[b"event"],
                )?,
            )
            .await?;
    }
    let prefix = store.config().subject_prefix();
    let correlation = broker_subjects(
        &context,
        &store,
        &format!("{prefix}.aggregate.*.*.flow%2E%2A%3E%25.*"),
    )
    .await?;
    let causation = broker_subjects(
        &context,
        &store,
        &format!("{prefix}.aggregate.*.*.*.cause%2E%2A%3E%25"),
    )
    .await?;
    let expected_correlation = vec![
        format!("{prefix}.aggregate.Fleet.first.flow%2E%2A%3E%25.cause%2E%2A%3E%25"),
        format!("{prefix}.aggregate.Reservation.second.flow%2E%2A%3E%25.other-cause"),
    ];
    let expected_causation = vec![
        format!("{prefix}.aggregate.Fleet.first.flow%2E%2A%3E%25.cause%2E%2A%3E%25"),
        format!("{prefix}.aggregate.Fleet.third.other-flow.cause%2E%2A%3E%25"),
    ];
    context.delete_stream(store.config().stream_name()).await?;
    assert_eq!(
        (correlation, causation),
        (expected_correlation, expected_causation)
    );
    Ok(())
}

#[tokio::test]
async fn full_history_and_directory_span_changing_correlation_and_causation() -> TestResult<()> {
    let (context, store) = directory_fixture("subject-history").await?;
    let aggregate = stream("history")?;
    let mut expected_events = Vec::new();
    let mut expected_subjects = Vec::new();
    let mut first_batch = None;
    for (index, (correlation, causation, suffix)) in [
        (None, None, "~.~"),
        (Some("flow-one"), Some("cause-one"), "flow-one.cause-one"),
        (Some("flow-two"), Some("cause-two"), "flow-two.cause-two"),
        (Some("~"), Some("~"), "%7E.%7E"),
    ]
    .into_iter()
    .enumerate()
    {
        let operation = format!("history-{index}");
        let events = correlated_batch(
            &aggregate,
            &operation,
            correlation,
            causation,
            &[b"one", b"two"],
        )?;
        if first_batch.is_none() {
            first_batch = Some(events.clone());
        }
        let expected_version = if expected_events.is_empty() {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(u64::try_from(expected_events.len())?))
        };
        let appended = store.append(&aggregate, expected_version, events).await?;
        expected_events.extend_from_slice(appended.events());
        for _ in appended.events() {
            expected_subjects.push(format!(
                "{}.aggregate.IntegrationAggregate.history.{suffix}",
                store.config().subject_prefix()
            ));
        }
        let unrelated = StreamId::new(
            AggregateType::new("OtherAggregate")?,
            AggregateId::new(format!("noise-{index}"))?,
        );
        store
            .append(
                &unrelated,
                ExpectedVersion::NoStream,
                batch(&unrelated, &operation, &operation, &[b"noise"])?,
            )
            .await?;
    }
    assert_eq!(store.load(&aggregate).await?, expected_events);
    assert_eq!(store.audit_history(&aggregate).await?, expected_events);
    assert_eq!(
        store.append_session().await?.load(&aggregate).await?,
        expected_events
    );
    assert_eq!(
        store.list_streams(aggregate.aggregate_type()).await?,
        vec![StreamSummary::new(aggregate.clone(), StreamVersion::new(8))]
    );
    let replay = store
        .append(
            &aggregate,
            ExpectedVersion::NoStream,
            first_batch.ok_or("missing first batch")?,
        )
        .await?;
    assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
    assert_eq!(
        replay.events(),
        expected_events.get(..2).ok_or("missing initial events")?
    );
    let subjects = broker_subjects(
        &context,
        &store,
        &format!(
            "{}.aggregate.IntegrationAggregate.history.*.*",
            store.config().subject_prefix()
        ),
    )
    .await?;
    context.delete_stream(store.config().stream_name()).await?;
    assert_eq!(
        subjects, expected_subjects,
        "one aggregate history spans every metadata subject"
    );
    Ok(())
}

#[tokio::test]
async fn stale_sessions_use_aggregate_wide_cas_across_metadata_subjects() -> TestResult<()> {
    let (context, store) = directory_fixture("subject-session-cas").await?;
    let mut actual_expectations = Vec::new();
    let mut expected_expectations = Vec::new();
    for seeded in [false, true] {
        let aggregate = stream(if seeded { "existing" } else { "absent" })?;
        let expected_version = if seeded {
            store
                .append(
                    &aggregate,
                    ExpectedVersion::NoStream,
                    correlated_batch(
                        &aggregate,
                        "seed",
                        Some("seed-flow"),
                        Some("seed-cause"),
                        &[b"seed"],
                    )?,
                )
                .await?;
            ExpectedVersion::Exact(StreamVersion::new(1))
        } else {
            ExpectedVersion::NoStream
        };
        let stale = store.append_session().await?;
        let replay_session = store.append_session().await?;
        let before = stale.load(&aggregate).await?;
        assert_eq!(replay_session.load(&aggregate).await?, before);
        let winner = correlated_batch(
            &aggregate,
            "winner",
            Some("winner-flow"),
            Some("winner-cause"),
            &[b"one", b"two"],
        )?;
        let appended = store
            .append(&aggregate, expected_version, winner.clone())
            .await?;
        let journal = context.get_stream(store.config().stream_name()).await?;
        let winning_first_sequence = journal
            .cached_info()
            .state
            .last_sequence
            .checked_sub(1)
            .ok_or("missing winning batch")?;
        let first = journal.get_raw_message(winning_first_sequence).await?;
        actual_expectations.push(
            first
                .headers
                .get(EXPECTATION_SUBJECT_HEADER)
                .map(ToString::to_string),
        );
        expected_expectations.push(Some(format!(
            "{}.aggregate.IntegrationAggregate.{}.*.*",
            store.config().subject_prefix(),
            aggregate.aggregate_id()
        )));
        let messages_before = journal.cached_info().state.messages;
        let result = stale
            .append(
                &aggregate,
                expected_version,
                correlated_batch(
                    &aggregate,
                    "loser",
                    Some("loser-flow"),
                    Some("loser-cause"),
                    &[b"must-not-commit"],
                )?,
            )
            .await;
        assert!(matches!(result, Err(error) if error.kind() == EventStoreErrorKind::Conflict));
        let replay = replay_session
            .append(&aggregate, expected_version, winner)
            .await?;
        assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
        assert_eq!(replay.events(), appended.events());
        let mut expected_history = before;
        expected_history.extend_from_slice(appended.events());
        assert_eq!(store.load(&aggregate).await?, expected_history);
        assert_eq!(
            context
                .get_stream(store.config().stream_name())
                .await?
                .cached_info()
                .state
                .messages,
            messages_before
        );
    }
    context.delete_stream(store.config().stream_name()).await?;
    assert_eq!(
        actual_expectations, expected_expectations,
        "native CAS must cover all correlation and causation subjects"
    );
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn atomic_multi_aggregate_writes_and_read_guards_cover_all_metadata_subjects()
-> TestResult<()> {
    let (context, store) = directory_fixture("subject-transactions").await?;
    let writer = stream("writer")?;
    let secondary = stream("secondary")?;
    let guard = stream("guard")?;
    store
        .append(
            &guard,
            ExpectedVersion::NoStream,
            correlated_batch(
                &guard,
                "guard-seed",
                Some("seed-flow"),
                Some("seed-cause"),
                &[b"seed"],
            )?,
        )
        .await?;
    let stale = store.append_session().await?;
    for aggregate in [&writer, &secondary, &guard] {
        stale.load(aggregate).await?;
    }
    let transaction = |operation: &str,
                       correlation: &str,
                       causation: &str,
                       guard_version|
     -> TestResult<EventTransaction> {
        Ok(EventTransaction::new(
            OperationId::new(operation)?,
            ContentFingerprint::digest(operation),
            vec![
                TransactionParticipant::new(
                    guard.clone(),
                    ExpectedVersion::Exact(StreamVersion::new(guard_version)),
                    None,
                ),
                TransactionParticipant::new(
                    writer.clone(),
                    ExpectedVersion::NoStream,
                    Some(correlated_batch(
                        &writer,
                        operation,
                        Some(correlation),
                        Some(causation),
                        &[b"one", b"two"],
                    )?),
                ),
                TransactionParticipant::new(
                    secondary.clone(),
                    ExpectedVersion::NoStream,
                    Some(correlated_batch(
                        &secondary,
                        operation,
                        Some(correlation),
                        Some(causation),
                        &[b"three"],
                    )?),
                ),
            ],
        )
        .with_correlation_id(CorrelationId::new(correlation)?)
        .with_causation_id(CausationId::new(causation)?))
    };
    store
        .append(
            &guard,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            correlated_batch(
                &guard,
                "guard-change",
                Some("changed-flow"),
                Some("changed-cause"),
                &[b"changed"],
            )?,
        )
        .await?;
    let before = context
        .get_stream(store.config().stream_name())
        .await?
        .cached_info()
        .state
        .messages;
    let rejected = stale
        .append_transaction(transaction(
            "stale-transaction",
            "stale-flow",
            "stale-cause",
            1,
        )?)
        .await;
    assert!(matches!(rejected, Err(error) if error.kind() == EventStoreErrorKind::Conflict));
    assert!(store.load(&writer).await?.is_empty());
    assert!(store.load(&secondary).await?.is_empty());
    assert!(
        store
            .load_transaction_receipt(&OperationId::new("stale-transaction")?)
            .await?
            .is_none()
    );
    assert_eq!(
        context
            .get_stream(store.config().stream_name())
            .await?
            .cached_info()
            .state
            .messages,
        before
    );

    let committed = transaction(
        "committed-transaction",
        "committed-flow",
        "committed-cause",
        2,
    )?;
    let accepted = store.append_transaction(committed.clone()).await?;
    let replay = store.append_transaction(committed).await?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.receipt(), accepted.receipt());
    for aggregate in [&writer, &secondary] {
        let expected: Vec<_> = accepted
            .receipt()
            .events()
            .into_iter()
            .filter(|event| event.stream_id() == aggregate)
            .collect();
        assert_eq!(store.load(aggregate).await?, expected);
        assert_eq!(store.audit_history(aggregate).await?, expected);
    }
    let journal = context.get_stream(store.config().stream_name()).await?;
    let guard_message = journal
        .get_last_raw_message_by_subject(
            &store
                .config()
                .transaction_guard_subject("committed-transaction", 0),
        )
        .await?;
    let mut actual_expectations = vec![
        guard_message
            .headers
            .get(EXPECTATION_SUBJECT_HEADER)
            .map(ToString::to_string),
    ];
    let prefix = store.config().subject_prefix();
    let mut expected_expectations = vec![Some(format!(
        "{prefix}.aggregate.IntegrationAggregate.guard.*.*"
    ))];
    // Inspect the first event of each writing participant by envelope identity;
    // this assertion also runs against the baseline's opaque stored subjects.
    for aggregate in [&writer, &secondary] {
        let expected_event = accepted
            .receipt()
            .events()
            .into_iter()
            .find(|event| event.stream_id() == aggregate)
            .ok_or("missing participant event")?;
        for sequence in checked_add_u64(before, 1, "first transaction sequence")?
            ..=journal.cached_info().state.last_sequence
        {
            let message = journal.get_raw_message(sequence).await?;
            let envelope: serde_json::Value = serde_json::from_slice(&message.payload)?;
            if envelope
                .pointer("/event/eventId")
                .and_then(serde_json::Value::as_str)
                == Some(expected_event.event_id().as_str())
            {
                actual_expectations.push(
                    message
                        .headers
                        .get(EXPECTATION_SUBJECT_HEADER)
                        .map(ToString::to_string),
                );
                expected_expectations.push(Some(format!(
                    "{prefix}.aggregate.IntegrationAggregate.{}.*.*",
                    aggregate.aggregate_id()
                )));
            }
        }
    }
    assert_eq!(
        actual_expectations.len(),
        3,
        "read guard and both writing participants were inspected"
    );
    let subjects = broker_subjects(
        &context,
        &store,
        &format!("{prefix}.aggregate.*.*.committed-flow.committed-cause"),
    )
    .await?;
    let expected_subjects = vec![
        format!("{prefix}.aggregate.IntegrationAggregate.writer.committed-flow.committed-cause"),
        format!("{prefix}.aggregate.IntegrationAggregate.writer.committed-flow.committed-cause"),
        format!("{prefix}.aggregate.IntegrationAggregate.secondary.committed-flow.committed-cause"),
    ];
    context.delete_stream(store.config().stream_name()).await?;
    assert_eq!(
        (actual_expectations, subjects),
        (expected_expectations, expected_subjects)
    );
    Ok(())
}
