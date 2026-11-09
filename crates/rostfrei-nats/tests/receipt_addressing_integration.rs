//! Run with `cargo xtask test`: its disposable broker also cleans up failed assertions.
#![allow(
    clippy::panic_in_result_fn,
    reason = "acceptance assertions retain native setup and transport errors through TestResult"
)]

mod terminal_puback_support;

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream::{self, message::StreamMessage};
use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventStore,
    EventStoreErrorKind, EventTransaction, ExpectedVersion, FixtureEventStore, NewEvent,
    OperationId, StreamId, StreamVersion, TransactionParticipant, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::{ApplicationName, CausationId, CorrelationId, TrafficScope};
use rostfrei_nats::{
    NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, connect, provision_event_store,
};
use terminal_puback_support::TerminalPubAckLossProxy;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const EXPECTATION_SUBJECT: &str = "Nats-Expected-Last-Subject-Sequence-Subject";
static NEXT_APPLICATION: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    js: jetstream::Context,
    store: NatsEventStore,
}

impl Fixture {
    async fn new(scope: TrafficScope) -> TestResult<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let sequence = NEXT_APPLICATION.fetch_add(1, Ordering::Relaxed);
        let application = ApplicationName::new(format!(
            "receipts-{:x}-{nanos:x}-{sequence:x}",
            std::process::id()
        ))?;
        let context = application.bounded_context_in_scope(scope, "sales")?;
        let config = NatsEventStoreConfig::for_bounded_context(&context)?
            .with_fresh_split_outcome_epoch("receipt-addressing")?
            .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
        let connection = connect(&NatsConnectionConfig::new(
            "receipt-addressing-tests",
            rostfrei_testing::integration::nats_url()?,
        ))
        .await?;
        let js = connection.jetstream().clone();
        provision_event_store(&js, &config).await?;
        let store = NatsEventStore::connect(js.clone(), config).await?;
        Ok(Self { js, store })
    }

    async fn journal(&self) -> TestResult<Vec<StreamMessage>> {
        let stream = self
            .js
            .get_stream(self.store.config().stream_name())
            .await?;
        let mut messages = Vec::new();
        for sequence in 1..=stream.cached_info().state.last_sequence {
            messages.push(stream.get_raw_message(sequence).await?);
        }
        Ok(messages)
    }

    async fn cleanup(self) -> TestResult {
        self.js
            .delete_stream(self.store.config().stream_name())
            .await?;
        self.js
            .delete_stream(self.store.config().outcome_state_stream_name())
            .await?;
        Ok(())
    }
}

fn aggregate(id: &str) -> TestResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new("sales/order")?,
        AggregateId::new(id)?,
    ))
}

fn batch(
    stream: &StreamId,
    operation: &str,
    count: u32,
    correlation: Option<&str>,
    causation: Option<&str>,
) -> TestResult<EventBatch> {
    let operation_id = OperationId::new(operation)?;
    let commit = derive_commit_id(stream, &operation_id);
    let events = (0..count)
        .map(|ordinal| {
            NewEvent::new(
                derive_event_id(&commit, ordinal),
                "order-changed",
                1,
                ordinal.to_be_bytes().to_vec(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut batch = EventBatch::new(
        commit,
        operation_id,
        ContentFingerprint::digest(operation),
        events,
    )?;
    if let Some(correlation) = correlation {
        batch = batch.with_correlation_id(CorrelationId::new(correlation)?);
    }
    if let Some(causation) = causation {
        batch = batch.with_causation_id(CausationId::new(causation)?);
    }
    Ok(batch)
}

fn transaction(
    store: &NatsEventStore,
    stream: &StreamId,
    operation: &str,
    expected: ExpectedVersion,
    count: u32,
    correlation: &str,
    causation: &str,
) -> TestResult<EventTransaction> {
    let participants = if count == 0 {
        Vec::new()
    } else {
        vec![TransactionParticipant::new(
            stream.clone(),
            expected,
            Some(batch(
                stream,
                operation,
                count,
                Some(correlation),
                Some(causation),
            )?),
        )]
    };
    Ok(EventTransaction::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(operation),
        participants,
    )
    .with_bounded_context(store.config().bounded_context().clone())
    .with_correlation_id(CorrelationId::new(correlation)?)
    .with_causation_id(CausationId::new(causation)?))
}

// Guard addressing is deliberately unchanged by the receipt move. Reuse its
// operation digest without duplicating the production digest algorithm in this test.
fn operation_prefix(config: &NatsEventStoreConfig, operation: &str) -> TestResult<String> {
    Ok(config
        .transaction_guard_subject(operation, 0)
        .strip_suffix(".guard.0")
        .ok_or("guard subject has no ordinal suffix")?
        .to_owned())
}

fn header<'a>(message: &'a StreamMessage, name: &str) -> Option<&'a str> {
    message
        .headers
        .get(name)
        .map(async_nats::HeaderValue::as_str)
}

#[tokio::test]
async fn aggregate_subject_is_stable_and_receipts_address_both_control_ids() -> TestResult {
    let fixture = Fixture::new(TrafficScope::Normal).await?;
    let stream = aggregate("order.42")?;
    let mut recorded = Vec::new();
    let mut expected_subjects = Vec::new();
    for (operation, version, correlation, causation, encoded_correlation, encoded_causation) in [
        (
            "first",
            ExpectedVersion::NoStream,
            "request.one/*",
            "command.one/>",
            "request%2Eone%2F%2A",
            "command%2Eone%2F%3E",
        ),
        (
            "second",
            ExpectedVersion::Exact(StreamVersion::new(1)),
            "request.two/~",
            "command.two/%",
            "request%2Etwo%2F%7E",
            "command%2Etwo%2F%25",
        ),
    ] {
        let accepted = fixture
            .store
            .append_transaction(transaction(
                &fixture.store,
                &stream,
                operation,
                version,
                1,
                correlation,
                causation,
            )?)
            .await?;
        assert_eq!(
            fixture
                .store
                .load_transaction_receipt(&OperationId::new(operation)?)
                .await?
                .as_ref(),
            Some(accepted.receipt()),
            "operation lookup must find the authoritative receipt without control IDs"
        );
        recorded.extend(accepted.receipt().events());
        expected_subjects.push(format!(
            "{}.aggregate.sales%2Forder.order%2E42",
            fixture.store.config().subject_prefix()
        ));
        expected_subjects.push(format!(
            "{}.receipt.{encoded_correlation}.{encoded_causation}",
            operation_prefix(fixture.store.config(), operation)?
        ));
    }
    assert_eq!(fixture.store.load(&stream).await?, recorded);
    assert_eq!(fixture.store.audit_history(&stream).await?, recorded);
    let messages = fixture.journal().await?;
    let actual_subjects: Vec<_> = messages
        .iter()
        .map(|message| message.subject.to_string())
        .collect();
    fixture.cleanup().await?;
    assert_eq!(
        actual_subjects, expected_subjects,
        "correlation and causation belong on terminal receipts; aggregate addressing stays stable"
    );
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one native race scenario checks both eventful and event-free operation winners"
)]
async fn operation_wide_duplicate_protection_includes_event_free_decisions() -> TestResult {
    let fixture = Fixture::new(TrafficScope::Normal).await?;
    let mut actual_receipts = Vec::new();
    let mut expected_receipts = Vec::new();
    for count in [0, 1] {
        let operation = format!("race-{count}");
        let left = aggregate(&format!("left-{count}"))?;
        let right = aggregate(&format!("right-{count}"))?;
        let left_tx = transaction(
            &fixture.store,
            &left,
            &operation,
            ExpectedVersion::NoStream,
            count,
            "race.a",
            "cause.a",
        )?;
        let right_tx = transaction(
            &fixture.store,
            &right,
            &operation,
            ExpectedVersion::NoStream,
            count,
            "race.b",
            "cause.b",
        )?;
        let before = fixture.journal().await?.len();
        // Distinct aggregates ensure aggregate-version checks cannot protect this
        // operation identity. The zero-event case has no aggregate guard at all.
        let results: [_; 2] = tokio::join!(
            fixture.store.append_transaction(left_tx.clone()),
            fixture.store.append_transaction(right_tx.clone()),
        )
        .into();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(error) if error.kind() == EventStoreErrorKind::IdentityConflict))
                .count(),
            1,
            "changed correlation/causation cannot create another operation winner: {results:?}"
        );
        let (
            winner_tx,
            winner_stream,
            correlation,
            causation,
            encoded_correlation,
            encoded_causation,
        ) = if results.first().is_some_and(Result::is_ok) {
            (left_tx, &left, "race.a", "cause.a", "race%2Ea", "cause%2Ea")
        } else {
            (
                right_tx,
                &right,
                "race.b",
                "cause.b",
                "race%2Eb",
                "cause%2Eb",
            )
        };
        let winner = results
            .iter()
            .find_map(|result| result.as_ref().ok())
            .ok_or("race produced no winner")?;
        assert!(!winner.is_exact_replay());
        assert_eq!(
            fixture
                .store
                .load_transaction_receipt(&OperationId::new(&operation)?)
                .await?
                .as_ref(),
            Some(winner.receipt())
        );
        let replay = fixture.store.append_transaction(winner_tx).await?;
        assert!(replay.is_exact_replay());
        assert_eq!(replay.receipt(), winner.receipt());
        for (retry_correlation, retry_causation) in [
            ("changed.correlation", causation),
            (correlation, "changed.cause"),
        ] {
            let retry = fixture
                .store
                .append_transaction(transaction(
                    &fixture.store,
                    winner_stream,
                    &operation,
                    ExpectedVersion::NoStream,
                    count,
                    retry_correlation,
                    retry_causation,
                )?)
                .await;
            assert!(
                matches!(&retry, Err(error) if error.kind() == EventStoreErrorKind::IdentityConflict),
                "each control ID is evidence bound to the existing winner: {retry:?}"
            );
        }
        let histories = [
            fixture.store.load(&left).await?,
            fixture.store.load(&right).await?,
        ];
        assert_eq!(
            histories.iter().map(Vec::len).sum::<usize>(),
            usize::try_from(count)?
        );
        let journal = fixture.journal().await?;
        assert_eq!(
            journal.len().checked_sub(before),
            usize::try_from(count)?.checked_add(1),
            "only the winning events and one permanent receipt may be appended"
        );
        let receipt = journal.last().ok_or("missing terminal receipt")?;
        actual_receipts.push((
            receipt.subject.to_string(),
            header(receipt, EXPECTATION_SUBJECT).map(str::to_owned),
            header(receipt, "Nats-Expected-Last-Subject-Sequence").map(str::to_owned),
        ));
        let prefix = operation_prefix(fixture.store.config(), &operation)?;
        expected_receipts.push((
            format!("{prefix}.receipt.{encoded_correlation}.{encoded_causation}"),
            Some(format!("{prefix}.receipt.*.*")),
            Some("0".to_owned()),
        ));
    }
    fixture.cleanup().await?;
    assert_eq!(
        actual_receipts, expected_receipts,
        "receipt CAS0 must cover every correlation/causation address for the operation"
    );
    Ok(())
}

#[tokio::test]
async fn test_fixture_batches_keep_identity_and_gain_atomic_terminal_locators() -> TestResult {
    let fixture = Fixture::new(TrafficScope::Test).await?;
    let operation = OperationId::new("shared-fixture-operation")?;
    let mut expected_batches = Vec::new();
    for (id, correlation, causation, suffix) in [
        (
            "fixture-one",
            Some("fixture.corr/*"),
            Some("fixture.cause/>"),
            ".receipt.fixture%2Ecorr%2F%2A.fixture%2Ecause%2F%3E",
        ),
        ("fixture-two", None, None, ".receipt.~.~"),
    ] {
        let stream = aggregate(id)?;
        let events = batch(&stream, operation.as_str(), 100, correlation, causation)?;
        let appended = fixture
            .store
            .append_fixture(&stream, ExpectedVersion::NoStream, events.clone())
            .await?;
        assert!(matches!(appended, AppendOutcome::Appended(_)));
        let replay = fixture
            .store
            .append_fixture(&stream, ExpectedVersion::NoStream, events)
            .await?;
        assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
        assert_eq!(replay.events(), appended.events());
        let commit = derive_commit_id(&stream, &operation);
        assert_eq!(
            appended.events().len(),
            100,
            "the fixture limit remains 100 events"
        );
        for (ordinal, event) in appended.events().iter().enumerate() {
            assert_eq!(event.commit_id(), &commit);
            assert_eq!(
                event.event_id(),
                &derive_event_id(&commit, u32::try_from(ordinal)?)
            );
        }
        assert_eq!(fixture.store.load(&stream).await?, appended.events());
        assert_eq!(
            fixture.store.audit_history(&stream).await?,
            appended.events()
        );
        expected_batches.push((appended.into_events(), suffix));
    }
    assert!(
        fixture
            .store
            .load_transaction_receipt(&operation)
            .await?
            .is_none(),
        "fixture manifests must not become command decisions"
    );
    assert_eq!(
        fixture
            .js
            .get_stream(fixture.store.config().outcome_state_stream_name())
            .await?
            .cached_info()
            .state
            .messages,
        0,
        "fixture manifests do not prepare command result details"
    );
    let journal = fixture.journal().await?;
    let fixture_prefix = format!("{}.fixture.", fixture.store.config().subject_prefix());
    fixture.cleanup().await?;
    let locators: Vec<_> = journal
        .iter()
        .filter(|message| message.subject.as_str().starts_with(&fixture_prefix))
        .collect();
    assert_eq!(
        locators.len(),
        2,
        "each Test fixture batch needs its own terminal locator; observed {} journal records",
        journal.len()
    );
    assert_eq!(
        journal.len(),
        202,
        "200 domain events and two fixture manifests, no replay writes"
    );
    let mut locator_identities = Vec::new();
    for (events, suffix) in expected_batches {
        let locator = locators
            .iter()
            .find(|message| message.subject.as_str().ends_with(suffix))
            .ok_or("fixture locator does not encode its optional control IDs")?;
        let identity = locator
            .subject
            .as_str()
            .strip_prefix(&fixture_prefix)
            .and_then(|value| value.strip_suffix(suffix))
            .ok_or("invalid fixture locator address")?;
        assert!(!identity.is_empty() && !identity.contains('.'));
        locator_identities.push(identity);
        assert_eq!(header(locator, "Nats-Batch-Commit"), Some("1"));
        assert_eq!(header(locator, "Nats-Batch-Sequence"), Some("101"));
        let batch_id = header(locator, "Nats-Batch-Id").ok_or("locator has no atomic batch ID")?;
        for event in events {
            let stored = journal
                .iter()
                .find(|message| header(message, "Nats-Msg-Id") == Some(event.event_id().as_str()))
                .ok_or("fixture event was not stored with its original identity")?;
            assert_eq!(header(stored, "Nats-Batch-Id"), Some(batch_id));
            assert_eq!(header(stored, "Nats-Batch-Commit"), None);
            assert!(stored.sequence < locator.sequence);
        }
    }
    assert_ne!(
        locator_identities.first(),
        locator_identities.last(),
        "the same fixture operation on two aggregates needs distinct locator identities"
    );
    Ok(())
}

#[tokio::test]
async fn normal_scope_rejects_direct_fixture_injection_before_mutation() -> TestResult {
    let fixture = Fixture::new(TrafficScope::Normal).await?;
    let stream = aggregate("normal-injection")?;
    let result = fixture
        .store
        .append_fixture(
            &stream,
            ExpectedVersion::NoStream,
            batch(&stream, "normal-injection", 1, None, None)?,
        )
        .await;
    let history = fixture.store.load(&stream).await?;
    let journal = fixture.journal().await?;
    fixture.cleanup().await?;
    assert_eq!(
        (result.is_err(), history.len(), journal.len()),
        (true, 0, 0),
        "Normal scope must reject direct fixture injection before persistence; result: {result:?}"
    );
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one recovery scenario covers lost PubAck, later writes, restart, and read-only replay"
)]
async fn fixture_reconciles_lost_terminal_ack_and_replays_after_later_writes() -> TestResult {
    let fixture = Fixture::new(TrafficScope::Test).await?;
    let proxy =
        TerminalPubAckLossProxy::start(fixture.store.config().stream_name().to_owned()).await?;
    let upstream: async_nats::ServerAddr = rostfrei_testing::integration::nats_url()?.parse()?;
    let options = upstream
        .username()
        .map_or_else(async_nats::ConnectOptions::new, |username| {
            async_nats::ConnectOptions::with_user_and_password(
                username.to_owned(),
                upstream.password().unwrap_or_default().to_owned(),
            )
        });
    let client = options.connect(format!("nats://{}", proxy.address)).await?;
    let proxied = NatsEventStore::connect(
        jetstream::new(client),
        fixture
            .store
            .config()
            .clone()
            .with_puback_timeout(Duration::from_millis(200))?,
    )
    .await?;
    let stream = aggregate("ack-loss")?;
    let first = batch(
        &stream,
        "uncertain-fixture",
        2,
        Some("first-flow"),
        Some("first-cause"),
    )?
    .with_actor(rostfrei_core::ActorRef::new(
        "test-operators",
        "fixture-author",
    )?);
    let recovered = proxied
        .append_fixture(&stream, ExpectedVersion::NoStream, first.clone())
        .await?;
    assert_eq!(
        proxy.dropped.load(Ordering::SeqCst),
        1,
        "the real terminal broker PubAck was lost"
    );
    assert!(matches!(recovered, AppendOutcome::ExactReplay(_)));
    assert!(
        recovered
            .events()
            .iter()
            .all(|event| event.actor() == first.actor())
    );
    drop(proxied);
    drop(proxy);
    fixture
        .store
        .append_fixture(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(2)),
            batch(&stream, "later-fixture", 1, Some("later-flow"), None)?,
        )
        .await?;
    let restarted =
        NatsEventStore::connect(fixture.js.clone(), fixture.store.config().clone()).await?;
    let replay = restarted
        .append_fixture(&stream, ExpectedVersion::NoStream, first)
        .await?;
    assert!(matches!(replay, AppendOutcome::ExactReplay(_)));
    assert_eq!(replay.events(), recovered.events());
    let changed = restarted
        .append_fixture(
            &stream,
            ExpectedVersion::NoStream,
            batch(
                &stream,
                "uncertain-fixture",
                2,
                Some("changed-flow"),
                Some("first-cause"),
            )?,
        )
        .await;
    assert!(matches!(changed, Err(error) if error.kind() == EventStoreErrorKind::IdentityConflict));
    let read_only = NatsEventStore::connect(
        fixture.js.clone(),
        fixture.store.config().clone().with_read_only_outcomes(),
    )
    .await?;
    let denied = read_only
        .append_fixture(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(3)),
            batch(&stream, "read-only", 1, None, None)?,
        )
        .await;
    assert!(
        matches!(denied, Err(error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    assert_eq!(restarted.audit_history(&stream).await?.len(), 3);
    assert_eq!(
        fixture.journal().await?.len(),
        5,
        "exact retries and refused writes add neither events nor manifests"
    );
    fixture.cleanup().await
}

#[tokio::test]
async fn incompatible_journal_layout_is_rejected_without_mutation() -> TestResult {
    let fixture = Fixture::new(TrafficScope::Test).await?;
    let journal = fixture
        .js
        .get_stream(fixture.store.config().stream_name())
        .await?;
    let mut retired = journal.cached_info().config.clone();
    retired.subjects = vec![
        format!(
            "{}.aggregate.*.*.*.*",
            fixture.store.config().subject_prefix()
        ),
        fixture.store.config().transaction_subject_filter(),
    ];
    fixture.js.update_stream(retired.clone()).await?;
    let provision = provision_event_store(&fixture.js, fixture.store.config()).await;
    let update = rostfrei_nats::update_event_store(&fixture.js, fixture.store.config()).await;
    let connect = NatsEventStore::connect(fixture.js.clone(), fixture.store.config().clone()).await;
    let write = fixture
        .store
        .append_fixture(
            &aggregate("stale-layout")?,
            ExpectedVersion::NoStream,
            batch(&aggregate("stale-layout")?, "stale-layout", 1, None, None)?,
        )
        .await;
    for result in [provision, update, connect.map(|_| ()), write.map(|_| ())] {
        assert!(
            matches!(result, Err(error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
        );
    }
    let after = fixture
        .js
        .get_stream(fixture.store.config().stream_name())
        .await?;
    assert_eq!(after.cached_info().config, retired);
    assert_eq!(after.cached_info().state.messages, 0);
    fixture.cleanup().await
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one attachment follows native state deletion, replacement, and journal recreation"
)]
async fn history_attachment_survives_lost_state_but_rejects_writes_and_recreated_journals()
-> TestResult {
    use rostfrei_core::{
        CommandCompletion, CommandExecutionMetadata, ExecutionStore, StreamDirectory,
    };
    let fixture = Fixture::new(TrafficScope::Test).await?;
    let stream = aggregate("historical")?;
    let proposal = transaction(
        &fixture.store,
        &stream,
        "historical",
        ExpectedVersion::NoStream,
        1,
        "history-flow",
        "history-cause",
    )?;
    let metadata = CommandExecutionMetadata::new(
        proposal.operation_id().clone(),
        proposal.operation_fingerprint(),
    )
    .with_default_outcome_evidence::<()>();
    let proposal = proposal.with_completion(CommandCompletion::new(
        metadata.outcome_evidence().cloned(),
        None,
    ));
    fixture.store.commit_outcome(proposal.clone()).await?;
    let state_config = fixture
        .js
        .get_stream(fixture.store.config().outcome_state_stream_name())
        .await?
        .cached_info()
        .config
        .clone();
    fixture
        .js
        .delete_stream(fixture.store.config().outcome_state_stream_name())
        .await?;
    assert!(
        NatsEventStore::connect(fixture.js.clone(), fixture.store.config().clone())
            .await
            .is_err()
    );
    let history =
        NatsEventStore::connect_history(fixture.js.clone(), fixture.store.config().clone()).await?;
    assert_eq!(history.authority(), fixture.store.authority());
    assert_eq!(history.load(&stream).await?.len(), 1);
    assert_eq!(history.audit_history(&stream).await?.len(), 1);
    assert!(
        history
            .load_transaction_receipt(proposal.operation_id())
            .await?
            .is_some()
    );
    assert_eq!(
        history.list_streams(stream.aggregate_type()).await?.len(),
        1
    );
    assert!(history.append_session().await.is_err());
    assert!(history.begin_command().await.is_err());
    assert!(history.commit_outcome(proposal.clone()).await.is_err());
    assert!(
        history
            .append_transaction(transaction(
                &fixture.store,
                &stream,
                "new-command",
                ExpectedVersion::Exact(StreamVersion::new(1)),
                1,
                "new-flow",
                "new-cause"
            )?)
            .await
            .is_err()
    );
    assert!(
        history
            .append_fixture(
                &stream,
                ExpectedVersion::Exact(StreamVersion::new(1)),
                batch(&stream, "new-fixture", 1, None, None)?
            )
            .await
            .is_err()
    );
    fixture.js.create_stream(state_config).await?;
    assert_eq!(history.audit_history(&stream).await?.len(), 1);
    let journal = fixture
        .js
        .get_stream(fixture.store.config().stream_name())
        .await?;
    let original = journal.cached_info().config.clone();
    let mut changed = original.clone();
    changed.metadata.remove("rostfrei.split.state-incarnation");
    fixture.js.update_stream(changed).await?;
    assert!(
        matches!(history.load(&stream).await, Err(error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    assert!(
        NatsEventStore::connect_history(fixture.js.clone(), fixture.store.config().clone())
            .await
            .is_err()
    );
    fixture
        .js
        .delete_stream(fixture.store.config().stream_name())
        .await?;
    fixture.js.create_stream(original).await?;
    assert!(
        matches!(history.load(&stream).await, Err(error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    assert!(
        matches!(history.list_streams(stream.aggregate_type()).await, Err(error) if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    fixture.cleanup().await
}
