//! Independent release joins: real configured host, historical meanings, and current authority.
//! Run with `scripts/test_nats.py`; never point this target at an application broker.
#![cfg(feature = "nats-host")]
#![allow(
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use async_trait::async_trait;
use rostfrei::{
    Aggregate, AggregateCodecs, ApplicationName, AuditPhase, AuditTerminal, AuthorizationError,
    CommandAccess, CommandAuthorizer, CommandDecision, CommandExecution, CommandHandler,
    CommandHandlingResult, CommittedDomainEvent, ContentFingerprint, EventBatch, EventCodec,
    EventCodecError, EventCodecErrorKind, EventId, ExpectedVersion, InMemoryCommandAuditSink,
    IntegrationEvent, IntegrationEventMapper, NewEvent, OperationId, OutcomeKey, OutcomeLookup,
    OutcomeReader, RecordedEvent, StreamAggregateId, StreamAggregateType, StreamId,
    TrustedCommandContext, ValidatedCommand,
};
use rostfrei_core::{ConflictRetry, ConflictRetryDecision, ConflictRetryPolicy, EventStore};
use rostfrei_messaging_core::{CommandAddress, MessageDelivery};
use rostfrei_nats::{NatsConnection, NatsConnectionConfig, TrustedCommandContextResolver, connect};
use rostfrei_tracer::{
    ApplicationBindings, CommandRequest, ExposeTracePayloadsForLocalDevelopment, Fixture,
    HandlerSimulationError, NatsApplication, NatsExecutionProfile, NatsRuntime, NatsRuntimeConfig,
    ObservationFeed, ObservationScope, SubmissionError, TestScenarioReset, Tracer, TracerConfig,
    http::HttpConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Notify;

rostfrei::install_macro_support!();
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(rostfrei::BoundedContext)]
#[domain(id = "boundary", label = "Boundary acceptance")]
struct Boundary;

#[derive(rostfrei::Command)]
#[domain(context = Boundary, id = "adjust", label = "Adjust")]
struct Adjust {
    delta: i64,
    reject: bool,
    no_event: bool,
    pause: bool,
}

#[derive(Serialize, Deserialize, rostfrei::DomainEvent)]
#[domain(id = "changed", label = "Changed")]
struct Changed {
    value: i64,
}

#[derive(rostfrei::AggregateEvents)]
enum Changes {
    Changed(Changed),
}

struct Ledger;
impl Aggregate for Ledger {
    type State = i64;
    type Event = Changes;
    const BOUNDED_CONTEXT: &'static str = "boundary";
    const AGGREGATE_TYPE: &'static str = "boundary/ledger";
    fn initial(_: &StreamId) -> i64 {
        0
    }
    fn apply(state: &mut i64, event: &Changes) {
        let Changes::Changed(event) = event;
        *state = event.value;
    }
}

// Neither representation is the generated Changed { value } representation.
// v1 whole units and v2 minor units deliberately have different meanings.
struct HistoricalCodec;
impl EventCodec<Ledger> for HistoricalCodec {
    fn encode(&self, event: &Changes, id: EventId) -> Result<NewEvent, EventCodecError> {
        let Changes::Changed(event) = event;
        NewEvent::new(
            id,
            "changed",
            2,
            format!("{{\"minor\":{}}}", event.value).into_bytes(),
        )
        .map_err(|error| codec_error(error.to_string()))
    }
    fn decode(&self, event: &RecordedEvent) -> Result<Changes, EventCodecError> {
        let payload: Value = serde_json::from_slice(event.payload())
            .map_err(|error| codec_error(error.to_string()))?;
        let value = match event.schema_version() {
            1 => payload["whole"].as_i64().and_then(|n| n.checked_mul(100)),
            2 => payload["minor"].as_i64(),
            _ => None,
        }
        .ok_or_else(|| codec_error("invalid historical money"))?;
        Ok(Changes::Changed(Changed { value }))
    }
}
fn codec_error(message: impl Into<String>) -> EventCodecError {
    EventCodecError::new(EventCodecErrorKind::MalformedPayload, message)
}

#[derive(rostfrei::DomainError)]
#[domain(
    id = "refused",
    label = "Refused",
    code = "REFUSED",
    message = "Refused"
)]
struct Refused {
    balance: i64,
}

#[derive(Clone, Debug)]
struct Evaluation {
    operation: OperationId,
    balance: i64,
}
#[derive(Default)]
struct Pause {
    armed: AtomicBool,
    entered: Notify,
    resume: Notify,
}
struct AdjustHandler {
    evaluations: Arc<Mutex<Vec<Evaluation>>>,
    pause: Arc<Pause>,
}
#[async_trait]
impl CommandHandler<Adjust> for AdjustHandler {
    type Rejection = Refused;
    async fn handle(
        &self,
        command: &Adjust,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Refused> {
        let operation = execution.metadata().operation_id().clone();
        let mut ledger = execution.load::<Ledger>("one").await?;
        let balance = *ledger.state();
        self.evaluations
            .lock()
            .unwrap()
            .push(Evaluation { operation, balance });
        if command.pause && self.pause.armed.swap(false, Ordering::SeqCst) {
            self.pause.entered.notify_one();
            self.pause.resume.notified().await;
        }
        if !command.no_event {
            ledger.raise(Changes::Changed(Changed {
                value: balance.saturating_add(command.delta),
            }));
        }
        Ok(if command.reject {
            CommandDecision::Rejected(Refused { balance })
        } else {
            CommandDecision::Accepted
        })
    }
}

#[derive(Serialize, Deserialize)]
struct PublishedBalance {
    value: i64,
}
impl IntegrationEvent for PublishedBalance {
    const EVENT_NAME: &'static str = "published-balance";
    const SCHEMA_VERSION: u32 = 1;
}
struct PublishBalance(Arc<Mutex<Vec<i64>>>);
impl IntegrationEventMapper<Changed> for PublishBalance {
    type Output = PublishedBalance;
    fn map(&self, event: &CommittedDomainEvent<'_, Changed>) -> PublishedBalance {
        self.0.lock().unwrap().push(event.event().value);
        PublishedBalance {
            value: event.event().value,
        }
    }
}

#[derive(Default)]
struct Policy {
    deny_sender: AtomicBool,
    accesses: Mutex<Vec<(String, CommandAccess)>>,
}
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        _: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        assert_eq!(caller.security_scope().as_str(), "acceptance");
        let principal = caller.principal().as_str();
        self.accesses
            .lock()
            .unwrap()
            .push((principal.to_owned(), access));
        if principal == "boundary-sender" && self.deny_sender.load(Ordering::SeqCst) {
            Err(AuthorizationError::Denied)
        } else {
            Ok(())
        }
    }
}
struct WorkerIdentity;
#[async_trait]
impl TrustedCommandContextResolver for WorkerIdentity {
    async fn resolve(
        &self,
        _: &MessageDelivery<CommandAddress>,
    ) -> Result<TrustedCommandContext, AuthorizationError> {
        Ok(caller("boundary-worker"))
    }
}
fn caller(principal: &str) -> TrustedCommandContext {
    TrustedCommandContext::new(
        rostfrei::PrincipalId::new(principal).unwrap(),
        rostfrei::SecurityScope::new("acceptance").unwrap(),
    )
}
#[derive(Default)]
struct RetryPolicy(Mutex<Vec<usize>>);
impl ConflictRetryPolicy for RetryPolicy {
    fn after_conflict(&self, conflict: ConflictRetry) -> ConflictRetryDecision {
        self.0.lock().unwrap().push(conflict.evaluations());
        if conflict.evaluations() == 1 {
            ConflictRetryDecision::RetryNow
        } else {
            ConflictRetryDecision::Stop
        }
    }
}

#[derive(Default)]
struct Witnesses {
    evaluations: Arc<Mutex<Vec<Evaluation>>>,
    published: Arc<Mutex<Vec<i64>>>,
    pause: Arc<Pause>,
    policy: Arc<Policy>,
    retries: Arc<RetryPolicy>,
}
fn bindings(witnesses: &Witnesses) -> TestResult<Arc<ApplicationBindings>> {
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Ledger, _>(HistoricalCodec)?;
    let mut bindings = ApplicationBindings::new().with_event_codecs(Arc::new(codecs));
    bindings.register_aggregate::<Ledger>()?;
    bindings.register_command::<Adjust, _>(
        AdjustHandler {
            evaluations: witnesses.evaluations.clone(),
            pause: witnesses.pause.clone(),
        },
        rostfrei::JsonDomainRejectionMapper::new(
            rostfrei::CommandRejectionClassification::Conflict,
        ),
    )?;
    bindings.register_integration_event::<Ledger, Changed, _>(PublishBalance(
        witnesses.published.clone(),
    ));
    Ok(Arc::new(bindings))
}
fn fixture(id: &str, payload: &Value) -> TestResult<Fixture> {
    Ok(serde_json::from_value(
        json!({"schemaVersion":1,"id":id,"revision":"v1", "messages":[{
            "kind":"domain-event","messageId":format!("{id}-event"),"correlationId":"fixture-history",
        "name":"changed","schemaVersion":1,"aggregate":{"type":"boundary/ledger","id":"one"},
            "streamVersion":1,"payload":payload
        }]}),
    )?)
}
fn stream() -> TestResult<StreamId> {
    Ok(StreamId::new(
        StreamAggregateType::new("boundary/ledger")?,
        StreamAggregateId::new("one")?,
    ))
}
fn request(reject: bool, no_event: bool, pause: bool) -> CommandRequest {
    CommandRequest {
        schema_version: 1,
        payload: json!({"delta":7,"reject":reject,"no_event":no_event,"pause":pause}),
    }
}
fn configuration(
    context: rostfrei::BoundedContext,
    bindings: &ApplicationBindings,
    witnesses: &Witnesses,
    audit: Arc<InMemoryCommandAuditSink>,
) -> TestResult<NatsRuntimeConfig> {
    let mut config = NatsRuntimeConfig::new(context, bindings, 1)?
        .with_fresh_completed_outcome_epoch("independent-boundary")?;
    let policy = witnesses.policy.clone();
    let retries = witnesses.retries.clone();
    config.execution_profile = NatsExecutionProfile::configured(
        move |builder| {
            builder
                .authorizer(policy.clone())
                .audit_sink(audit.clone())
                .conflict_retry_policy(retries.clone())
                .rejection_codec(Arc::new(rostfrei_core::JsonRejectionCodec::new(
                    "boundary.refusal",
                    73,
                )))
        },
        Arc::new(WorkerIdentity),
        caller("boundary-sender"),
    );
    Ok(config)
}
async fn connection(label: &str) -> TestResult<(NatsConnection, ApplicationName)> {
    let connection = connect(&NatsConnectionConfig::new(
        label,
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new(format!(
        "{label}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ))?;
    Ok((connection, application))
}
async fn submit(tracer: &Tracer, id: &str, request: CommandRequest) -> TestResult<Value> {
    let queued = tracer
        .submit_test("boundary", "adjust", request, Some(id))
        .await?;
    let mut events = tracer.subscribe(&queued.operation_id, 0).await?;
    tokio::time::timeout(Duration::from_secs(35), async {
        while events.next().await.is_some() {}
    })
    .await?;
    Ok(serde_json::to_value(
        tracer.operation(&queued.operation_id).await?,
    )?)
}
fn key(runtime: &NatsRuntime, id: &str) -> TestResult<OutcomeKey> {
    Ok(OutcomeKey::new(
        runtime.store().authority().clone(),
        runtime.config().context.name().clone(),
        OperationId::new(id)?,
    ))
}

// Last sequence detects attempted append/delete as well as net message growth.
async fn native_sequences(
    connection: &NatsConnection,
    runtime: &NatsRuntime,
) -> TestResult<Vec<(u64, u64)>> {
    let config = runtime.config();
    let topology = config.messaging.topology();
    // Responses can reach Tracer before the worker's WorkQueue acknowledgement.
    // Wait for that independent transport housekeeping before taking a baseline.
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut commands = connection
            .jetstream()
            .get_stream(topology.command_stream().as_str())
            .await?;
        while commands.info().await?.state.messages != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        TestResult::Ok(())
    })
    .await??;
    let mut result = Vec::new();
    for name in [
        config.event_store.stream_name(),
        config.event_store.outcome_state_stream_name(),
        topology.command_stream().as_str(),
        topology.command_response_stream().as_str(),
        topology.integration_event_stream().as_str(),
    ] {
        let mut stream = connection.jetstream().get_stream(name).await?;
        let state = &stream.info().await?.state;
        result.push((state.messages, state.last_sequence));
    }
    Ok(result)
}
async fn await_integration(
    connection: &NatsConnection,
    runtime: &NatsRuntime,
    expected: u64,
) -> TestResult<Value> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = connection
            .jetstream()
            .get_stream(
                runtime
                    .config()
                    .messaging
                    .topology()
                    .integration_event_stream()
                    .as_str(),
            )
            .await?;
        loop {
            if stream.info().await?.state.messages >= expected {
                return Ok(serde_json::from_slice(
                    &stream.get_raw_message(expected).await?.payload,
                )?);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?
}
async fn assert_rejection_codec(
    runtime: &NatsRuntime,
    operation: &Value,
    balance: i64,
) -> TestResult {
    assert_eq!(operation["status"], "completed", "{operation}");
    assert_eq!(operation["result"]["decision"], "rejected", "{operation}");
    let OutcomeLookup::Completed(outcome) = runtime
        .store()
        .lookup_outcome(&key(
            runtime,
            operation["operationId"]
                .as_str()
                .ok_or("missing operation")?,
        )?)
        .await?
    else {
        return Err("missing durable rejection".into());
    };
    let rejection = outcome
        .details()
        .available()?
        .rejection()
        .ok_or("missing encoded rejection")?;
    assert_eq!(rejection.codec_id, "boundary.refusal");
    assert_eq!(rejection.codec_version, 73);
    let decoded: rostfrei::CommandRejection = serde_json::from_slice(&rejection.bytes)?;
    assert_eq!(
        decoded.details().ok_or("missing rejection details")?["balance"],
        balance
    );
    Ok(())
}

async fn assert_diagnostics_are_read_only(
    connection: &NatsConnection,
    runtime: &NatsRuntime,
    tracer: &Tracer,
    witnesses: &Witnesses,
    feed: &ObservationFeed,
    balance: i64,
) -> TestResult {
    let before = native_sequences(connection, runtime).await?;
    let history = runtime.store().load(&stream()?).await?;
    let observation = serde_json::to_value(feed.snapshot())?;
    let first = witnesses.evaluations.lock().unwrap().len();
    for (reject, no_event) in [(false, false), (true, false), (false, true)] {
        let result = serde_json::to_value(
            tracer
                .simulate_handler("boundary", "adjust", request(reject, no_event, false))
                .await?,
        )?;
        assert_eq!(result["kind"], "handlerSimulation");
        assert_eq!(result["committed"], false);
        assert_eq!(result["streams"][0]["baseStreamVersion"], 2);
        assert_eq!(
            result["outcome"]["decision"],
            if reject { "rejected" } else { "accepted" }
        );
        if reject {
            assert_eq!(result["outcome"]["rejection"]["balance"], balance);
        }
        if reject || no_event {
            assert_eq!(result["proposedEvents"], json!([]));
        } else {
            assert_eq!(
                result["proposedEvents"][0]["payload"],
                json!({"minor":balance.saturating_add(7)})
            );
            assert_eq!(result["proposedEvents"][0]["schemaVersion"], 2);
            for name in ["messageId", "causationId", "correlationId"] {
                assert!(result["proposedEvents"][0].get(name).is_none());
            }
        }
        for name in [
            "operationId",
            "correlationId",
            "messageSeries",
            "messageSeriesHref",
            "receipt",
        ] {
            assert!(
                result.get(name).is_none(),
                "diagnostics must not issue {name}"
            );
        }
    }
    let diagnostics = witnesses.evaluations.lock().unwrap()[first..].to_vec();
    assert_eq!(diagnostics.len(), 3);
    for evaluation in &diagnostics {
        assert_eq!(evaluation.balance, balance);
        assert!(matches!(
            tracer.operation(evaluation.operation.as_str()).await,
            Err(SubmissionError::NotFound)
        ));
        assert!(matches!(
            runtime
                .store()
                .lookup_outcome(&key(runtime, evaluation.operation.as_str())?)
                .await?,
            OutcomeLookup::Missing
        ));
    }
    assert!(matches!(
        tracer
            .submit_simulation(
                "boundary",
                "adjust",
                request(false, false, false),
                Some("retired-preview")
            )
            .await,
        Err(SubmissionError::ModeUnavailable("simulate"))
    ));
    assert_eq!(
        witnesses.evaluations.lock().unwrap().len(),
        first.saturating_add(3)
    );
    assert_eq!(
        native_sequences(connection, runtime).await?,
        before,
        "diagnostics must not append to any of the five native streams"
    );
    assert_eq!(runtime.store().load(&stream()?).await?, history);
    assert_eq!(serde_json::to_value(feed.snapshot())?, observation);
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn configured_custom_meanings_and_diagnostic_isolation_survive_paired_resets() -> TestResult {
    let (connection, application) = connection("boundary-codecs").await?;
    let witnesses = Witnesses::default();
    let bindings = bindings(&witnesses)?;
    let audit = Arc::new(InMemoryCommandAuditSink::new(256));
    let context = application.test_bounded_context("boundary")?;
    let config = configuration(context.clone(), &bindings, &witnesses, audit.clone())?;
    let feed = ObservationFeed::new(application, ObservationScope::Test);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let tracer_config =
        TracerConfig::new(listener, HttpConfig::new("owned-boundary-test-capability")?)
            .with_continuous_observation(feed.clone())
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    let invalid = fixture("invalid", &json!({"wrong":2}))?;
    let host = NatsApplication::builder(context, connection.clone(), bindings)?
        .with_runtime_config(config)
        .with_default_test_fixture(fixture("two", &json!({"whole":2}))?)
        .with_test_fixture(fixture("three", &json!({"whole":3}))?)
        .with_test_fixture(fixture("four", &json!({"whole":4}))?)
        .with_test_fixture(invalid.clone())
        .with_tracer(Some(tracer_config))
        .provision_and_start()
        .await?;
    let tracer = host.tracer().ok_or("missing configured Tracer")?;
    let runtime = host.runtime();
    for (fixture_id, units) in [("two", 2_i64), ("three", 3), ("four", 4)] {
        if fixture_id != "two" {
            tracer.reset_test_scenario_with_fixture(fixture_id).await?;
        }
        audit.clear().await;
        witnesses.policy.accesses.lock().unwrap().clear();
        let published_before = witnesses.published.lock().unwrap().len();
        let initial = runtime.store().load(&stream()?).await?;
        assert_eq!(initial.len(), 1);
        assert_eq!(
            initial[0].payload(),
            format!("{{\"whole\":{units}}}").as_bytes()
        );
        let evaluated_before = witnesses.evaluations.lock().unwrap().len();
        let accepted = submit(tracer, "real-adjustment", request(false, false, false)).await?;
        assert_eq!(accepted["status"], "completed", "{accepted}");
        assert_eq!(accepted["result"]["decision"], "accepted");
        let balance = units.saturating_mul(100).saturating_add(7);
        assert_eq!(
            witnesses.evaluations.lock().unwrap()[evaluated_before].balance,
            units.saturating_mul(100)
        );
        let integration = await_integration(&connection, runtime, 1).await?;
        assert_eq!(integration["payload"]["value"], balance, "{integration}");
        assert_eq!(
            &witnesses.published.lock().unwrap()[published_before..],
            &[balance],
            "fixture must not be privately dispatched"
        );
        let history = runtime.store().load(&stream()?).await?;
        assert_eq!(&history[..1], &initial);
        assert_eq!(history[1].schema_version(), 2);
        assert_eq!(
            history[1].payload(),
            format!("{{\"minor\":{balance}}}").as_bytes()
        );
        // Canonical fixture node IDs become deterministic native identities. Reapply
        // through the public fixture API and preserve the whole recorded envelopes.
        let reused = runtime
            .apply_fixture(&fixture(fixture_id, &json!({"whole": units}))?)
            .await?;
        assert_eq!(reused.applied_domain_event_count(), 0);
        assert_eq!(reused.reused_domain_event_count(), 1);
        assert_eq!(runtime.store().load(&stream()?).await?, history);
        // Ensure the observation feed has caught up before checking diagnostic immutability.
        tracer
            .operation_message_series(
                accepted["operationId"].as_str().ok_or("operation id")?,
                "5s".parse()?,
                "100ms".parse()?,
            )
            .await?;
        assert_diagnostics_are_read_only(&connection, runtime, tracer, &witnesses, &feed, balance)
            .await?;
        let rejected = submit(tracer, "real-refusal", request(true, false, false)).await?;
        assert_rejection_codec(runtime, &rejected, balance).await?;
        assert_eq!(
            runtime.store().load(&stream()?).await?,
            history,
            "rejection must discard proposed events"
        );
        let accesses = witnesses.policy.accesses.lock().unwrap().clone();
        let worker: Vec<_> = accesses
            .iter()
            .filter(|(principal, _)| principal == "boundary-worker")
            .map(|(_, access)| *access)
            .collect();
        assert_eq!(
            worker,
            [
                CommandAccess::ReadOutcome,
                CommandAccess::Execute,
                CommandAccess::ReadOutcome,
                CommandAccess::Execute
            ]
        );
        assert!(accesses.contains(&("boundary-sender".to_owned(), CommandAccess::ReadOutcome)));
        let records = audit.records().await;
        let completed: Vec<_> = records
            .iter()
            .filter(|record| record.terminal() == Some(AuditTerminal::Committed))
            .collect();
        assert_eq!(completed.len(), 2);
        for record in completed {
            assert_eq!(
                record.principal().as_str(),
                "boundary-worker",
                "resolver identity must reach configured audit"
            );
            assert_eq!(record.security_scope().as_str(), "acceptance");
            assert_eq!(record.phase(), AuditPhase::Finished);
            let started = records
                .iter()
                .find(|other| {
                    other.attempt_id() == record.attempt_id()
                        && other.phase() == AuditPhase::Started
                })
                .ok_or("missing started audit")?;
            assert_eq!(started.original_payload(), record.original_payload());
            assert_eq!(started.command_id(), record.command_id());
        }
        let before_denial = native_sequences(&connection, runtime).await?;
        assert_eq!(
            before_denial[4].0, 1,
            "only the accepted command emits an integration event"
        );
        assert_eq!(
            &witnesses.published.lock().unwrap()[published_before..],
            &[balance]
        );
        let evaluations = witnesses.evaluations.lock().unwrap().len();
        witnesses.policy.deny_sender.store(true, Ordering::SeqCst);
        let denied = submit(tracer, "denied-fresh", request(false, false, false)).await?;
        assert_eq!(denied["status"], "failed", "{denied}");
        assert_eq!(native_sequences(&connection, runtime).await?, before_denial);
        assert_eq!(witnesses.evaluations.lock().unwrap().len(), evaluations);
        assert!(
            audit
                .records()
                .await
                .iter()
                .any(|record| record.principal().as_str() == "boundary-sender"
                    && record.terminal() == Some(AuditTerminal::Denied))
        );
        witnesses.policy.deny_sender.store(false, Ordering::SeqCst);
        assert!(
            witnesses.retries.0.lock().unwrap().is_empty(),
            "ordinary execution/diagnostics must not consume conflict budget"
        );
    }
    // Selected release deferral: direct runtime reset does not update Tracer diagnostic health.
    // Report the actual result, without treating diagnostic availability here as a release gate.
    assert!(runtime.reset(&invalid).await.is_err());
    assert!(runtime.start_workers().await.is_err());
    let direct = tracer
        .simulate_handler("boundary", "adjust", request(false, true, false))
        .await;
    eprintln!(
        "DEFERRED direct-runtime failed-reset diagnostic: {}",
        if direct.is_ok() {
            "still available"
        } else {
            "refused"
        }
    );
    // The advertised Tracer-mediated boundary IS mandatory and must recover.
    assert!(
        tracer
            .reset_test_scenario_with_fixture("invalid")
            .await
            .is_err()
    );
    assert!(matches!(
        tracer
            .simulate_handler("boundary", "adjust", request(false, true, false))
            .await,
        Err(HandlerSimulationError::Submission(
            SubmissionError::TestScenarioUnavailable
        ))
    ));
    tracer.reset_test_scenario().await?;
    let recovered = submit(tracer, "after-recovery", request(false, false, false)).await?;
    assert_eq!(recovered["status"], "completed", "{recovered}");
    host.shutdown().await?;
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one controlled conflict on each side of a paired reset"
)]
async fn configured_retry_reloads_a_real_native_conflict_before_and_after_reset() -> TestResult {
    let (connection, application) = connection("boundary-retry").await?;
    let witnesses = Witnesses::default();
    let bindings = bindings(&witnesses)?;
    let audit = Arc::new(InMemoryCommandAuditSink::new(128));
    let config = configuration(
        application.test_bounded_context("boundary")?,
        &bindings,
        &witnesses,
        audit.clone(),
    )?;
    let runtime = Arc::new(NatsRuntime::provision(connection.clone(), config, bindings).await?);
    let fixture = fixture("retry", &json!({"whole":2}))?;
    runtime.apply_fixture(&fixture).await?;
    runtime.start_workers().await?;
    let tracer = runtime
        .tracer_builder()
        .with_test_transport(runtime.transport())
        .with_test_scenario_reset(runtime.clone())
        .with_default_test_fixture(fixture)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
        .build()?;
    for cycle in 0..2 {
        if cycle != 0 {
            tracer.reset_test_scenario().await?;
        }
        witnesses.evaluations.lock().unwrap().clear();
        witnesses.retries.0.lock().unwrap().clear();
        witnesses.policy.accesses.lock().unwrap().clear();
        audit.clear().await;
        witnesses.pause.armed.store(true, Ordering::SeqCst);
        let pending = {
            let tracer = tracer.clone();
            tokio::spawn(async move {
                submit(&tracer, "conflicted-refusal", request(true, false, true)).await
            })
        };
        tokio::time::timeout(Duration::from_secs(10), witnesses.pause.entered.notified()).await?;
        let competing_operation = OperationId::new("independent-competing-write")?;
        let commit = rostfrei_core::derive_commit_id(&stream()?, &competing_operation);
        runtime
            .store()
            .append(
                &stream()?,
                ExpectedVersion::Exact(rostfrei::StreamVersion::new(1)),
                EventBatch::new(
                    commit.clone(),
                    competing_operation,
                    ContentFingerprint::digest("competing-write"),
                    vec![NewEvent::new(
                        rostfrei_core::derive_event_id(&commit, 0),
                        "changed",
                        2,
                        b"{\"minor\":900}".to_vec(),
                    )?],
                )?,
            )
            .await?;
        witnesses.pause.resume.notify_one();
        let rejected = pending.await??;
        assert_rejection_codec(&runtime, &rejected, 900).await?;
        assert_eq!(
            *witnesses.retries.0.lock().unwrap(),
            [1],
            "the supplied retry policy must see the genuine conflict"
        );
        assert_eq!(
            witnesses
                .evaluations
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.balance)
                .collect::<Vec<_>>(),
            [200, 900]
        );
        let accesses = witnesses.policy.accesses.lock().unwrap().clone();
        let executes = accesses
            .iter()
            .filter(|(principal, access)| {
                principal == "boundary-worker" && *access == CommandAccess::Execute
            })
            .count();
        assert_eq!(
            executes, 1,
            "retry is inside one admitted worker invocation"
        );
        let records = audit.records().await;
        assert_eq!(
            records
                .iter()
                .filter(|record| record.terminal() == Some(AuditTerminal::Committed))
                .count(),
            1
        );
        let history = runtime.store().load(&stream()?).await?;
        assert_eq!(
            history.len(),
            2,
            "only fixture and independent competing write, no rejected proposal"
        );
        assert_eq!(history[0].payload(), b"{\"whole\":2}");
        assert_eq!(history[1].payload(), b"{\"minor\":900}");
    }
    runtime.stop_workers().await;
    connection.drain().await?;
    Ok(())
}
