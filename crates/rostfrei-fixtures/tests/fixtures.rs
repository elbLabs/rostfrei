#![allow(clippy::panic_in_result_fn)]

use std::{
    error::Error,
    sync::atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, AppendOutcome, ContentFingerprint, Event, EventBatch,
    EventCodecError, EventCodecErrorKind, EventHistory, EventStore, EventStoreError,
    ExecutionMetadata, ExpectedVersion, InMemoryEventStore, NewEvent, OperationId, RecordedEvent,
    StreamId, StreamVersion,
};
use rostfrei_fixtures::{Fixture, FixtureApplyError, FixtureEventSet, MessageSeriesEngine};
use rostfrei_messaging_core::CausationId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Eq, PartialEq)]
enum AccountEvent {
    Opened { amount: i64 },
    Credited { amount: i64 },
}

impl Event for AccountEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Opened { .. } => "account-opened",
            Self::Credited { .. } => "account-credited",
        }
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        let amount = match self {
            Self::Opened { amount } | Self::Credited { amount } => *amount,
        };
        serde_json::to_vec(&AmountPayload { amount }).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
        })
    }

    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        if event.schema_version() != 1 {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "account events require schema version 1",
            ));
        }
        let payload =
            serde_json::from_slice::<AmountPayload>(event.payload()).map_err(|error| {
                EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
            })?;
        match event.event_type() {
            "account-opened" => Ok(Self::Opened {
                amount: payload.amount,
            }),
            "account-credited" => Ok(Self::Credited {
                amount: payload.amount,
            }),
            event_type => Err(EventCodecError::new(
                EventCodecErrorKind::UnknownEventType,
                format!("unknown account event `{event_type}`"),
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct AmountPayload {
    amount: i64,
}

struct Account;

impl Aggregate for Account {
    type Event = AccountEvent;
    type State = i64;

    const AGGREGATE_TYPE: &'static str = "account";

    fn initial(_stream_id: &StreamId) -> Self::State {
        0
    }

    fn apply(state: &mut Self::State, event: &Self::Event) {
        let amount = match event {
            AccountEvent::Opened { amount } | AccountEvent::Credited { amount } => *amount,
        };
        *state = state.saturating_add(amount);
    }
}

static REPLAY_APPLY_COUNT: AtomicUsize = AtomicUsize::new(0);

struct ReplayAccount;

impl Aggregate for ReplayAccount {
    type Event = AccountEvent;
    type State = i64;

    const AGGREGATE_TYPE: &'static str = "replay-account";

    fn initial(_stream_id: &StreamId) -> Self::State {
        0
    }

    fn apply(state: &mut Self::State, event: &Self::Event) {
        REPLAY_APPLY_COUNT.fetch_add(1, Ordering::SeqCst);
        Account::apply(state, event);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn fixture(messages: Vec<Value>) -> Result<Fixture, serde_json::Error> {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "id": "standard-accounts",
        "revision": "revision-1",
        "messages": messages,
    }))
}

fn command(message_id: &str, correlation_id: &str, aggregate_type: &str, id: &str) -> Value {
    json!({
        "kind": "command",
        "messageId": message_id,
        "correlationId": correlation_id,
        "name": "open-account",
        "schemaVersion": 1,
        "aggregate": { "type": aggregate_type, "id": id },
        "payload": { "amount": 10 },
    })
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn domain_event(
    message_id: &str,
    correlation_id: &str,
    causation_id: &str,
    aggregate_type: &str,
    id: &str,
    stream_version: u64,
    name: &str,
    payload: Value,
) -> Value {
    json!({
        "kind": "domain-event",
        "messageId": message_id,
        "correlationId": correlation_id,
        "causationId": causation_id,
        "name": name,
        "schemaVersion": 1,
        "aggregate": { "type": aggregate_type, "id": id },
        "streamVersion": stream_version,
        "payload": payload,
    })
}

fn stream_id(aggregate_type: &str, id: &str) -> TestResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new(aggregate_type)?,
        AggregateId::new(id)?,
    ))
}

#[tokio::test]
async fn persists_only_domain_events_and_replays_them_through_the_typed_aggregate() -> TestResult {
    REPLAY_APPLY_COUNT.store(0, Ordering::SeqCst);
    let fixture = fixture(vec![
        command("command-1", "correlation-1", "replay-account", "one"),
        domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "replay-account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
        domain_event(
            "event-2",
            "correlation-1",
            "event-1",
            "replay-account",
            "one",
            2,
            "account-credited",
            json!({ "amount": 5 }),
        ),
        json!({
            "kind": "integration-event",
            "messageId": "integration-1",
            "correlationId": "correlation-1",
            "causationId": "event-2",
            "name": "account-announced",
            "schemaVersion": 1,
            "payload": { "accountId": "one" },
        }),
        json!({
            "kind": "command-outcome",
            "messageId": "outcome-1",
            "correlationId": "correlation-1",
            "causationId": "command-1",
            "outcome": { "status": "accepted" },
        }),
    ])?;
    let store = InMemoryEventStore::new();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<ReplayAccount>()?;

    let report = engine.apply(&store, &fixture).await?;
    let events = store.load(&stream_id("replay-account", "one")?).await?;
    let fixture_events = FixtureEventSet::new(std::slice::from_ref(&fixture))?;

    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| fixture_events.contains(event)));
    assert_eq!(
        events.first().map(RecordedEvent::event_type),
        Some("account-opened")
    );
    assert!(
        events
            .first()
            .and_then(RecordedEvent::causation_id)
            .is_some_and(|id| {
                id.as_str().starts_with("fixture-message:") && id.as_str() != "command-1"
            })
    );
    assert_eq!(
        events
            .last()
            .and_then(RecordedEvent::causation_id)
            .map(CausationId::as_str),
        events.first().map(|event| event.event_id().as_str())
    );
    assert_eq!(REPLAY_APPLY_COUNT.load(Ordering::SeqCst), 2);
    assert_eq!(report.fixture_id(), fixture.id());
    assert_eq!(report.fixture_revision(), fixture.revision());
    assert_eq!(report.total_provenance_message_count(), 5);
    assert_eq!(report.applied_domain_event_count(), 2);
    assert_eq!(report.reused_domain_event_count(), 0);
    assert_eq!(report.fixture(), &fixture);
    assert_eq!(report.fixture().messages().len(), 5);
    Ok(())
}

#[tokio::test]
async fn a_second_application_is_an_exact_prefix_reuse() -> TestResult {
    let fixture = fixture(vec![
        command("command-1", "correlation-1", "account", "one"),
        domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
    ])?;
    let store = InMemoryEventStore::new();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;

    let first = engine.apply(&store, &fixture).await?;
    let second = engine.apply(&store, &fixture).await?;
    let events = store.load(&stream_id("account", "one")?).await?;

    assert_eq!(first.applied_domain_event_count(), 1);
    assert_eq!(first.reused_domain_event_count(), 0);
    assert_eq!(second.applied_domain_event_count(), 0);
    assert_eq!(second.reused_domain_event_count(), 1);
    assert_eq!(events.len(), 1);
    Ok(())
}

#[tokio::test]
async fn a_fixture_remains_applied_after_business_history_extends_it() -> TestResult {
    let fixture = fixture(vec![
        command("command-1", "correlation-1", "account", "one"),
        domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
    ])?;
    let store = InMemoryEventStore::new();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;
    engine.apply(&store, &fixture).await?;
    append_business_event(&store, &stream_id("account", "one")?).await?;

    let report = engine.apply(&store, &fixture).await?;
    let events = store.load(&stream_id("account", "one")?).await?;
    let fixture_events = FixtureEventSet::new(std::slice::from_ref(&fixture))?;

    assert_eq!(report.applied_domain_event_count(), 0);
    assert_eq!(report.reused_domain_event_count(), 1);
    assert_eq!(events.len(), 2);
    assert!(fixture_events.contains(&events[0]));
    assert!(!fixture_events.contains(&events[1]));
    Ok(())
}

#[tokio::test]
async fn a_root_domain_event_needs_no_synthetic_command() -> TestResult {
    let fixture = fixture(vec![json!({
        "kind": "domain-event",
        "messageId": "event-1",
        "correlationId": "correlation-1",
        "name": "account-opened",
        "schemaVersion": 1,
        "aggregate": { "type": "account", "id": "one" },
        "streamVersion": 1,
        "payload": { "amount": 10 },
    })])?;
    let store = InMemoryEventStore::new();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;

    let report = engine.apply(&store, &fixture).await?;
    let events = store.load(&stream_id("account", "one")?).await?;

    assert_eq!(report.total_provenance_message_count(), 1);
    assert_eq!(report.applied_domain_event_count(), 1);
    assert_eq!(events.len(), 1);
    assert!(
        events
            .first()
            .is_some_and(|event| event.causation_id().is_none())
    );
    Ok(())
}

#[test]
fn rejects_unresolved_cycles_cross_correlation_and_parent_after_child() {
    let unresolved = fixture(vec![domain_event(
        "event-1",
        "correlation-1",
        "missing-command",
        "account",
        "one",
        1,
        "account-opened",
        json!({ "amount": 10 }),
    )]);
    assert!(unresolved.is_err());

    let parent_after_child = fixture(vec![
        domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
        command("command-1", "correlation-1", "account", "one"),
    ]);
    assert!(parent_after_child.is_err());

    let cycle = fixture(vec![
        json!({
            "kind": "command",
            "messageId": "command-1",
            "correlationId": "correlation-1",
            "causationId": "command-2",
            "name": "one",
            "schemaVersion": 1,
            "aggregate": { "type": "account", "id": "one" },
            "payload": {},
        }),
        json!({
            "kind": "command",
            "messageId": "command-2",
            "correlationId": "correlation-1",
            "causationId": "command-1",
            "name": "two",
            "schemaVersion": 1,
            "aggregate": { "type": "account", "id": "one" },
            "payload": {},
        }),
    ]);
    assert!(cycle.is_err());

    let cross_correlation = fixture(vec![
        command("command-1", "correlation-1", "account", "one"),
        domain_event(
            "event-1",
            "correlation-2",
            "command-1",
            "account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
    ]);
    assert!(cross_correlation.is_err());
}

#[test]
fn rejects_noncontiguous_versions_and_invalid_outcome_edges() {
    let versions = fixture(vec![
        command("command-1", "correlation-1", "account", "one"),
        domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
        domain_event(
            "event-2",
            "correlation-1",
            "event-1",
            "account",
            "one",
            3,
            "account-credited",
            json!({ "amount": 2 }),
        ),
    ]);
    assert!(versions.is_err());

    let outcome_parent = fixture(vec![
        command("command-1", "correlation-1", "account", "one"),
        domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
        json!({
            "kind": "command-outcome",
            "messageId": "outcome-1",
            "correlationId": "correlation-1",
            "causationId": "event-1",
            "outcome": { "status": "accepted" },
        }),
    ]);
    assert!(outcome_parent.is_err());

    let duplicate_outcome = fixture(vec![
        command("command-1", "correlation-1", "account", "one"),
        json!({
            "kind": "command-outcome",
            "messageId": "outcome-1",
            "correlationId": "correlation-1",
            "causationId": "command-1",
            "outcome": { "status": "accepted" },
        }),
        json!({
            "kind": "command-outcome",
            "messageId": "outcome-2",
            "correlationId": "correlation-1",
            "causationId": "command-1",
            "outcome": { "status": "accepted" },
        }),
    ]);
    assert!(duplicate_outcome.is_err());
}

#[tokio::test]
async fn unknown_codec_fails_before_any_stream_is_written() -> TestResult {
    let fixture = fixture(vec![
        command("known-command", "known-correlation", "account", "known"),
        domain_event(
            "known-event",
            "known-correlation",
            "known-command",
            "account",
            "known",
            1,
            "account-opened",
            json!({ "amount": 1 }),
        ),
        command(
            "unknown-command",
            "unknown-correlation",
            "unknown-account",
            "unknown",
        ),
        domain_event(
            "unknown-event",
            "unknown-correlation",
            "unknown-command",
            "unknown-account",
            "unknown",
            1,
            "account-opened",
            json!({ "amount": 1 }),
        ),
    ])?;
    let store = InMemoryEventStore::new();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;

    let result = engine.apply(&store, &fixture).await;

    assert!(matches!(
        result,
        Err(FixtureApplyError::UnknownAggregateCodec { .. })
    ));
    assert!(
        store
            .load(&stream_id("account", "known")?)
            .await?
            .is_empty()
    );
    assert!(
        store
            .load(&stream_id("unknown-account", "unknown")?)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn a_command_only_aggregate_requires_a_registered_codec() -> TestResult {
    let fixture = fixture(vec![command(
        "unknown-command",
        "unknown-correlation",
        "unknown-account",
        "unknown",
    )])?;
    let store = InMemoryEventStore::new();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;

    assert!(matches!(
        engine.apply(&store, &fixture).await,
        Err(FixtureApplyError::UnknownAggregateCodec { aggregate_type })
            if aggregate_type.as_str() == "unknown-account"
    ));
    Ok(())
}

#[tokio::test]
async fn malformed_and_wrong_typed_events_fail_before_writes() -> TestResult {
    for (name, schema_version, payload, expected_kind) in [
        (
            "account-opened",
            1,
            json!({ "amount": "not-a-number" }),
            EventCodecErrorKind::MalformedPayload,
        ),
        (
            "not-an-account-event",
            1,
            json!({ "amount": 10 }),
            EventCodecErrorKind::UnknownEventType,
        ),
        (
            "account-opened",
            2,
            json!({ "amount": 10 }),
            EventCodecErrorKind::UnsupportedSchemaVersion,
        ),
    ] {
        let mut event = domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "account",
            "one",
            1,
            name,
            payload,
        );
        if let Value::Object(fields) = &mut event {
            fields.insert("schemaVersion".to_owned(), Value::from(schema_version));
        }
        let fixture = fixture(vec![
            command("command-1", "correlation-1", "account", "one"),
            event,
        ])?;
        let store = InMemoryEventStore::new();
        let mut engine = MessageSeriesEngine::new();
        engine.register_json::<Account>()?;

        let result = engine.apply(&store, &fixture).await;

        assert!(matches!(
            result,
            Err(FixtureApplyError::Codec { source, .. }) if source.kind() == expected_kind
        ));
        assert!(store.load(&stream_id("account", "one")?).await?.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn a_conflicting_prefix_prevents_new_writes_to_every_stream() -> TestResult {
    let store = InMemoryEventStore::new();
    let first_stream = stream_id("account", "a")?;
    seed_conflicting_event(&store, &first_stream).await?;
    let fixture = fixture(vec![
        command("command-a", "correlation-a", "account", "a"),
        domain_event(
            "event-a",
            "correlation-a",
            "command-a",
            "account",
            "a",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
        command("command-b", "correlation-b", "account", "b"),
        domain_event(
            "event-b",
            "correlation-b",
            "command-b",
            "account",
            "b",
            1,
            "account-opened",
            json!({ "amount": 20 }),
        ),
    ])?;
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;

    let result = engine.apply(&store, &fixture).await;

    assert!(matches!(
        result,
        Err(FixtureApplyError::ConflictingHistory { .. })
    ));
    assert_eq!(store.load(&first_stream).await?.len(), 1);
    assert!(store.load(&stream_id("account", "b")?).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn applies_independent_contiguous_plans_to_multiple_streams() -> TestResult {
    let fixture = fixture(vec![
        command("command-a", "correlation-a", "account", "a"),
        domain_event(
            "event-a",
            "correlation-a",
            "command-a",
            "account",
            "a",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
        command("command-b", "correlation-b", "account", "b"),
        domain_event(
            "event-b",
            "correlation-b",
            "command-b",
            "account",
            "b",
            1,
            "account-opened",
            json!({ "amount": 20 }),
        ),
    ])?;
    let store = InMemoryEventStore::new();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;

    let report = engine.apply(&store, &fixture).await?;

    assert_eq!(report.applied_domain_event_count(), 2);
    assert_eq!(store.load(&stream_id("account", "a")?).await?.len(), 1);
    assert_eq!(store.load(&stream_id("account", "b")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn appends_cross_stream_causation_in_authored_order() -> TestResult {
    let fixture = fixture(vec![
        command("command-z", "correlation-1", "account", "z"),
        domain_event(
            "event-z",
            "correlation-1",
            "command-z",
            "account",
            "z",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
        domain_event(
            "event-a",
            "correlation-1",
            "event-z",
            "account",
            "a",
            1,
            "account-opened",
            json!({ "amount": 20 }),
        ),
    ])?;
    let store = OrderedEventStore::default();
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<Account>()?;

    engine.apply(&store, &fixture).await?;

    assert_eq!(store.append_count.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn deserialization_is_strict_and_round_trips_camel_case_documents() -> TestResult {
    let fixture = fixture(vec![
        command("command-1", "correlation-1", "account", "one"),
        domain_event(
            "event-1",
            "correlation-1",
            "command-1",
            "account",
            "one",
            1,
            "account-opened",
            json!({ "amount": 10 }),
        ),
    ])?;
    let value = serde_json::to_value(&fixture)?;
    let round_trip = serde_json::from_value::<Fixture>(value)?;
    assert_eq!(round_trip, fixture);

    let unknown_field = serde_json::from_value::<Fixture>(json!({
        "schemaVersion": 1,
        "id": "fixture",
        "revision": "one",
        "messages": [],
        "extra": true,
    }));
    assert!(unknown_field.is_err());
    let wrong_schema = serde_json::from_value::<Fixture>(json!({
        "schemaVersion": 2,
        "id": "fixture",
        "revision": "one",
        "messages": [],
    }));
    assert!(wrong_schema.is_err());
    for reserved_id in [".", ".."] {
        let reserved = serde_json::from_value::<Fixture>(json!({
            "schemaVersion": 1,
            "id": reserved_id,
            "revision": "one",
            "messages": [],
        }));
        assert!(reserved.is_err());
    }
    Ok(())
}

#[derive(Default)]
struct OrderedEventStore {
    inner: InMemoryEventStore,
    append_count: AtomicUsize,
}

#[async_trait]
impl EventHistory for OrderedEventStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(stream_id).await
    }
}

#[async_trait]
impl EventStore for OrderedEventStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        let index = self.append_count.fetch_add(1, Ordering::SeqCst);
        let expected_stream = ["account:z", "account:a"].get(index).copied();
        assert_eq!(expected_stream, Some(stream_id.to_string().as_str()));
        self.inner.append(stream_id, expected_version, batch).await
    }
}

async fn seed_conflicting_event(store: &InMemoryEventStore, stream_id: &StreamId) -> TestResult {
    let fingerprint = ContentFingerprint::digest(b"conflicting-seed");
    let metadata = ExecutionMetadata::new(
        stream_id.clone(),
        OperationId::new("conflicting-seed")?,
        fingerprint,
    );
    let event = NewEvent::new(
        metadata.event_id(0),
        "account-opened",
        1,
        serde_json::to_vec(&AmountPayload { amount: 999 })?,
    )?;
    let batch = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        fingerprint,
        vec![event],
    )?;
    let _outcome = store
        .append(stream_id, ExpectedVersion::NoStream, batch)
        .await?;
    Ok(())
}

async fn append_business_event(store: &InMemoryEventStore, stream_id: &StreamId) -> TestResult {
    let fingerprint = ContentFingerprint::digest(b"business-credit");
    let metadata = ExecutionMetadata::new(
        stream_id.clone(),
        OperationId::new(format!(
            "fixture:{}",
            ContentFingerprint::digest(b"spoofed-fixture-operation")
        ))?,
        fingerprint,
    );
    let event = NewEvent::new(
        metadata.event_id(0),
        "account-credited",
        1,
        serde_json::to_vec(&AmountPayload { amount: 5 })?,
    )?;
    let batch = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        fingerprint,
        vec![event],
    )?;
    let _outcome = store
        .append(
            stream_id,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            batch,
        )
        .await?;
    Ok(())
}
