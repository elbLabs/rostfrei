//! One registry shared by fixtures, executor/processor, Preview and private-event dispatch.
//! Future-API target; see docs/testing/modular-runtime-tdd.md.
#![allow(
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
#[path = "../../rostfrei/tests/modular_support/mod.rs"]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei_fixtures::{Fixture, MessageSeriesEngine};
use rostfrei_registry::DomainRegistry;
use rostfrei_tracer::{SimulationRequest, TracerBuilder};
use serde_json::json;
rostfrei::install_macro_support!();

/// Historical JSON object differs semantically from the generated JSON integer.
/// Current v2 output is deliberately non-JSON.
#[derive(Clone)]
struct HistoricalCodec {
    reads: Arc<AtomicUsize>,
}
impl EventCodec<Counter> for HistoricalCodec {
    fn encode(&self, e: &Added, id: EventId) -> Result<NewEvent, EventCodecError> {
        CentsCodec {
            scale: 100,
            decodes: self.reads.clone(),
        }
        .encode(e, id)
    }
    fn decode(&self, e: &RecordedEvent) -> Result<Added, EventCodecError> {
        if e.schema_version() == 1 {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let old: serde_json::Value = serde_json::from_slice(e.payload()).map_err(|error| {
                EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
            })?;
            return old["whole_units"]
                .as_i64()
                .map(|n| Added(n.saturating_mul(100)))
                .ok_or_else(|| {
                    EventCodecError::new(
                        EventCodecErrorKind::MalformedPayload,
                        "missing historical whole_units",
                    )
                });
        }
        CentsCodec {
            scale: 100,
            decodes: self.reads.clone(),
        }
        .decode(e)
    }
}
fn historical_fixture(payload: &serde_json::Value) -> TestResult<Fixture> {
    Ok(serde_json::from_value(
        json!({"schemaVersion":1,"id":"old-cents","revision":"v1","messages":[{"kind":"domain-event","messageId":"historical-cents","correlationId":"historical-flow","name":"added","schemaVersion":1,"aggregate":{"type":"counter","id":"one"},"streamVersion":1,"payload":payload}]}),
    )?)
}
struct PreviewHandler(Arc<Mutex<Vec<i64>>>);
#[async_trait]
impl CommandHandler<Change> for PreviewHandler {
    type Rejection = std::convert::Infallible;
    async fn handle(
        &self,
        command: &Change,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut counter = execution.load::<Counter>("one").await?;
        self.0.lock().unwrap().push(*counter.state());
        counter.raise(Added(command.amount));
        Ok(CommandDecision::Accepted)
    }
}
struct Observe(Arc<Mutex<Vec<i64>>>);
#[async_trait]
impl DomainEventHandler<Added> for Observe {
    async fn handle(
        &self,
        e: &CommittedDomainEvent<'_, Added>,
    ) -> Result<(), DomainEventHandlerError> {
        self.0.lock().unwrap().push(e.event().0);
        Ok(())
    }
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one acceptance scenario proves the same historical meaning across all consumers"
)]
async fn historical_meaning_and_original_facts_are_shared_across_all_consumers() -> TestResult {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Counter, _>(HistoricalCodec {
        reads: reads.clone(),
    })?;
    let codecs = Arc::new(codecs);
    let store = Arc::new(InMemoryEventStore::new());
    let fixture = historical_fixture(&json!({"whole_units":2}))?;
    let engine = MessageSeriesEngine::with_codecs(codecs.clone());
    engine.apply(store.as_ref(), &fixture).await?;
    let original = store.load(&stream()?).await?;
    assert_eq!(original.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(original[0].payload())?,
        json!({"whole_units":2})
    );
    assert!(
        reads.load(Ordering::SeqCst) > 0,
        "fixture must call the configured decoder"
    );
    let observations = Arc::new(Mutex::new(Vec::new()));
    let mut dispatcher = DomainEventDispatcher::with_codecs(codecs.clone());
    dispatcher.register::<Counter, Added, _>("added", Arc::new(Observe(observations.clone())))?;
    let execution = CommandExecutionMetadata::new(
        OperationId::new("simulate-cents")?,
        ContentFingerprint::digest("simulation"),
    )
    .with_bounded_context(context()?.name().clone());
    let outcome = CommandExecutor::with_codecs(store.clone(), codecs.clone())
        .simulate(
            &ChangeHandler(Arc::new(AtomicUsize::new(0))),
            execution,
            &Change {
                amount: 7,
                reject: true,
            },
        )
        .await?;
    assert_eq!(
        outcome.into_parts().0,
        CommandDecision::Rejected("balance=200".to_owned())
    );
    assert!(
        observations.lock().unwrap().is_empty(),
        "decoding is not private-event delivery"
    );
    let mut processor = CommandProcessor::builder(context()?, store.clone())
        .event_codecs(codecs.clone())
        .authorizer(Arc::new(rostfrei::AllowAllCommands))
        .build()?;
    processor.register::<Change, _>(ChangeHandler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
    let command = bus(baseline_processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
    )?)?
    .encode(request("write-current-cents", false)?)?;
    let caller = rostfrei::TrustedCommandContext::new(
        rostfrei::PrincipalId::new("service")?,
        rostfrei::SecurityScope::new("scope")?,
    );
    rostfrei::CommandProcessingService::process(&processor, &caller, &command)
        .await
        .into_result()?;
    let history = store.load(&stream()?).await?;
    assert_eq!(&history[..1], &original);
    assert_eq!(history[1].schema_version(), 2);
    assert_eq!(history[1].payload(), 7_i64.to_le_bytes());
    // Preview observes mixed v1/v2 state, uses opaque encoding, and never appends.
    let states = Arc::new(Mutex::new(Vec::new()));
    let mut registry = DomainRegistry::new();
    registry.register_command::<Change, PreviewHandler>()?;
    let mut builder = TracerBuilder::new(store.clone(), registry)
        .with_event_codecs(codecs.clone())
        .with_trace_payload_policy(Arc::new(
            rostfrei_tracer::ExposeTracePayloadsForLocalDevelopment,
        ));
    builder.register_json::<Change, _>(PreviewHandler(states.clone()))?;
    let tracer = builder.build()?;
    tracer
        .submit_simulation(
            "modular",
            "change",
            SimulationRequest {
                schema_version: 1,
                payload: json!({"amount":7,"reject":false}),
            },
            Some("opaque-preview"),
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut subscription = tracer.subscribe("opaque-preview", 0).await.unwrap();
        while subscription.next().await.is_some() {}
    })
    .await?;
    assert_eq!(*states.lock().unwrap(), [207]);
    let snapshot = serde_json::to_value(tracer.operation("opaque-preview").await?)?;
    // Both frozen display choices are valid: explicit opaque rendering or the existing typed
    // InvalidEventPayload display failure, after the custom codec has actually been used.
    if snapshot["status"] == "failed" {
        assert!(
            snapshot
                .to_string()
                .contains("event payload is not valid JSON"),
            "unexpected Preview failure: {snapshot}"
        );
    } else {
        assert_eq!(snapshot["status"], "completed");
        assert!(
            snapshot.to_string().contains("base64"),
            "opaque payload needs an explicit encoding tag: {snapshot}"
        );
    }
    assert_eq!(store.load(&stream()?).await?, history);
    assert!(observations.lock().unwrap().is_empty());
    for event in &history {
        assert_eq!(
            dispatcher.dispatch(event).await?,
            DomainEventDispatchOutcome::Handled
        );
    }
    assert_eq!(*observations.lock().unwrap(), [200, 7]);
    engine.apply(store.as_ref(), &fixture).await?;
    assert_eq!(
        store.load(&stream()?).await?,
        history,
        "fixture replay preserves historical bytes/identities and subsequent writes"
    );
    Ok(())
}

struct SameWireModel;
impl Aggregate for SameWireModel {
    type State = i64;
    type Event = Added;
    const BOUNDED_CONTEXT: &'static str = "modular";
    const AGGREGATE_TYPE: &'static str = "counter";
    fn initial(_: &StreamId) -> i64 {
        0
    }
    fn apply(state: &mut i64, e: &Added) {
        *state = state.saturating_add(e.0);
    }
}
#[tokio::test]
async fn registration_collisions_and_malformed_fixture_fail_before_any_write() -> TestResult {
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Counter, _>(HistoricalCodec {
        reads: Arc::new(AtomicUsize::new(0)),
    })?;
    assert!(
        codecs
            .register::<Counter, _>(HistoricalCodec {
                reads: Arc::new(AtomicUsize::new(0))
            })
            .is_err(),
        "same codec Rust type is still a duplicate instance"
    );
    assert!(
        codecs.register_json::<SameWireModel>().is_err(),
        "wire name cannot choose between models"
    );
    codecs.replace::<Counter, _>(HistoricalCodec {
        reads: Arc::new(AtomicUsize::new(0)),
    })?;
    let store = InMemoryEventStore::new();
    let error = MessageSeriesEngine::with_codecs(Arc::new(codecs))
        .apply(&store, &historical_fixture(&json!({"misspelled_units":2}))?)
        .await
        .unwrap_err();
    assert!(
        matches!(error, rostfrei_fixtures::FixtureApplyError::Codec { schema_version: 1, ref event_type, ref stream_id, .. } if event_type == "added" && stream_id == &stream().unwrap())
    );
    assert!(store.load(&stream()?).await?.is_empty());
    Ok(())
}

struct OtherCounter;
impl Aggregate for OtherCounter {
    type State = i64;
    type Event = Added;
    const BOUNDED_CONTEXT: &'static str = "modular";
    const AGGREGATE_TYPE: &'static str = "other-counter";
    fn initial(_: &StreamId) -> i64 {
        0
    }
    fn apply(s: &mut i64, e: &Added) {
        *s = s.saturating_add(e.0);
    }
}
struct OtherCodec;
impl EventCodec<OtherCounter> for OtherCodec {
    fn encode(&self, e: &Added, id: EventId) -> Result<NewEvent, EventCodecError> {
        NewEvent::new(id, "added", 2, format!("other:{}", e.0).into_bytes())
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, e.to_string()))
    }
    fn decode(&self, e: &RecordedEvent) -> Result<Added, EventCodecError> {
        std::str::from_utf8(e.payload())
            .ok()
            .and_then(|s| s.strip_prefix("other:"))
            .and_then(|s| s.parse().ok())
            .map(Added)
            .ok_or_else(|| {
                EventCodecError::new(
                    EventCodecErrorKind::MalformedPayload,
                    "wrong participant codec",
                )
            })
    }
}
struct TwoParticipants;
#[async_trait]
impl CommandHandler<()> for TwoParticipants {
    type Rejection = std::convert::Infallible;
    async fn handle(
        &self,
        (): &(),
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut first = execution.load::<Counter>("one").await?;
        let mut second = execution.load::<OtherCounter>("two").await?;
        first.raise(Added(7));
        second.raise(Added(11));
        Ok(CommandDecision::Accepted)
    }
}
#[tokio::test]
async fn multi_aggregate_commit_and_private_dispatch_use_each_participants_own_codec() -> TestResult
{
    let store = InMemoryEventStore::new();
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Counter, _>(CentsCodec {
        scale: 100,
        decodes: Arc::new(AtomicUsize::new(0)),
    })?;
    codecs.register::<OtherCounter, _>(OtherCodec)?;
    let codecs = Arc::new(codecs);
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("two-codecs")?,
        ContentFingerprint::digest("two"),
    )
    .with_bounded_context(context()?.name().clone());
    let CommandOutcome::Accepted(receipt) = CommandExecutor::with_codecs(store, codecs.clone())
        .execute(&TwoParticipants, metadata, &())
        .await?;
    let observations = Arc::new(Mutex::new(Vec::new()));
    let mut dispatcher = DomainEventDispatcher::with_codecs(codecs);
    dispatcher.register::<Counter, Added, _>("added", Arc::new(Observe(observations.clone())))?;
    dispatcher
        .register::<OtherCounter, Added, _>("added", Arc::new(Observe(observations.clone())))?;
    assert_eq!(receipt.events().len(), 2);
    for event in receipt.events() {
        if event.stream_id().aggregate_type().as_str() == "counter" {
            assert_eq!(event.payload(), 7_i64.to_le_bytes());
        } else {
            assert_eq!(event.payload(), b"other:11");
        }
        dispatcher.dispatch(event).await?;
    }
    let mut values = observations.lock().unwrap().clone();
    values.sort_unstable();
    assert_eq!(values, [7, 11]);
    Ok(())
}
