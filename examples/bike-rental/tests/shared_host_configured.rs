#![allow(clippy::panic_in_result_fn)]

use async_trait::async_trait;
use bike_rental::{
    demo::{demo_fixture, demo_stream},
    rental_fleet::{RentBicycle, RentBicycleHandler, RentalFleetAggregate},
};
use rostfrei::core::JsonEventCodec;
use rostfrei::nats::{NatsConnectionConfig, ServicePrincipalContext, connect};
use rostfrei::tracer::{
    ApplicationBindings, CommandRequest, ExposeTracePayloadsForLocalDevelopment,
    NatsExecutionProfile, NatsRuntime, NatsRuntimeConfig, TestScenarioReset,
};
use rostfrei::{
    Aggregate, AuditError, AuthorizationError, CommandAccess, CommandAuditRecord, CommandAuditSink,
    CommandAuthorizer, CommandDecision, CommandHandler, EventCodec, EventCodecError, EventId,
    NewEvent, OutcomeLifecycle, OutcomeReader, RecordedEvent, TrustedCommandContext,
    ValidatedCommand,
};
use serde_json::{Value, json};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct CountedRent(Arc<AtomicUsize>);
#[async_trait]
impl CommandHandler<RentBicycle> for CountedRent {
    type Rejection = <RentBicycleHandler as CommandHandler<RentBicycle>>::Rejection;
    async fn handle(
        &self,
        command: &RentBicycle,
        execution: &mut rostfrei::CommandExecution<'_>,
    ) -> rostfrei::CommandHandlingResult<Self::Rejection> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if command.bicycle_id.as_str() == "no-event" {
            return Ok(CommandDecision::Accepted);
        }
        RentBicycleHandler.handle(command, execution).await
    }
}

struct Policy {
    revoked: AtomicBool,
    reads: AtomicUsize,
    executes: AtomicUsize,
}
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        _: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        assert_eq!(caller.principal().as_str(), "host-service");
        match access {
            CommandAccess::ReadOutcome => {
                self.reads.fetch_add(1, Ordering::SeqCst);
            }
            CommandAccess::Execute => {
                self.executes.fetch_add(1, Ordering::SeqCst);
            }
        }
        if self.revoked.load(Ordering::SeqCst) {
            Err(AuthorizationError::Denied)
        } else {
            Ok(())
        }
    }
}
struct Audit(Arc<AtomicUsize>);
#[async_trait]
impl CommandAuditSink for Audit {
    async fn record(&self, record: &CommandAuditRecord) -> Result<(), AuditError> {
        assert_eq!(record.principal().as_str(), "host-service");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
struct CountedCodec(Arc<AtomicUsize>);
impl EventCodec<RentalFleetAggregate> for CountedCodec {
    fn encode(
        &self,
        event: &<RentalFleetAggregate as Aggregate>::Event,
        id: EventId,
    ) -> Result<NewEvent, EventCodecError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        <JsonEventCodec as EventCodec<RentalFleetAggregate>>::encode(&JsonEventCodec, event, id)
    }
    fn decode(
        &self,
        event: &RecordedEvent,
    ) -> Result<<RentalFleetAggregate as Aggregate>::Event, EventCodecError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        <JsonEventCodec as EventCodec<RentalFleetAggregate>>::decode(&JsonEventCodec, event)
    }
}

async fn submit(runtime: &NatsRuntime, id: &str, bicycle: &str) -> TestResult<Value> {
    // Recreate only the in-memory Tracer, retaining the native authority and original key.
    let tracer = runtime
        .tracer_builder()
        .with_test_transport(runtime.transport())
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
        .build()?;
    let queued = tracer
        .submit_test("bike-rental", "rent-bicycle", request(bicycle), Some(id))
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
fn request(bicycle: &str) -> CommandRequest {
    CommandRequest {
        schema_version: <RentBicycle as rostfrei::Command>::SCHEMA_VERSION,
        payload: json!({"fleet_id":"city-fleet", "bicycle_id":bicycle}),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn configured_service_rebuilds_and_recreated_tracer_replays_truthfully_then_denies_expired_or_revoked()
-> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "configured-host",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let context = rostfrei::ApplicationName::new(format!(
        "configured-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ))?
    .test_bounded_context("bike-rental")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let codecs_used = Arc::new(AtomicUsize::new(0));
    let audits = Arc::new(AtomicUsize::new(0));
    let rebuilt = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy {
        revoked: AtomicBool::new(false),
        reads: AtomicUsize::new(0),
        executes: AtomicUsize::new(0),
    });
    let mut codecs = rostfrei::AggregateCodecs::default();
    codecs.register::<RentalFleetAggregate, _>(CountedCodec(codecs_used.clone()))?;
    let mut bindings = ApplicationBindings::new().with_event_codecs(Arc::new(codecs));
    bindings.register_aggregate::<RentalFleetAggregate>()?;
    bindings.register_command::<RentBicycle, _>(
        CountedRent(calls.clone()),
        rostfrei::JsonDomainRejectionMapper::new(
            rostfrei::CommandRejectionClassification::Conflict,
        ),
    )?;
    let caller = TrustedCommandContext::new(
        rostfrei::PrincipalId::new("host-service")?,
        rostfrei::SecurityScope::new("owned")?,
    );
    let mut config = NatsRuntimeConfig::new(context.clone(), &bindings, 1)?
        .with_fresh_completed_outcome_epoch("configured")?;
    config.event_store = config.event_store.with_explicit_detail_expiry(true)?;
    config.execution_profile = NatsExecutionProfile::configured(
        {
            let policy = policy.clone();
            let audits = audits.clone();
            let rebuilt = rebuilt.clone();
            move |builder| {
                rebuilt.fetch_add(1, Ordering::SeqCst);
                builder
                    .authorizer(policy.clone())
                    .audit_sink(Arc::new(Audit(audits.clone())))
                    .rejection_codec(Arc::new(rostfrei::core::JsonRejectionCodec::new(
                        "host.rejection",
                        17,
                    )))
                    .conflict_retry_policy(Arc::new(rostfrei::core::BoundedConflictRetries::new(0)))
            }
        },
        Arc::new(ServicePrincipalContext(caller.clone())),
        caller,
    );
    let runtime = NatsRuntime::provision(connection.clone(), config, Arc::new(bindings)).await?;
    for cycle in 0..2 {
        runtime.reset(&demo_fixture()?).await?;
        assert_eq!(rebuilt.load(Ordering::SeqCst), cycle + 2);
        let before = calls.load(Ordering::SeqCst);
        for (id, bicycle, decision) in [
            ("accepted", "bike-42", "accepted"),
            ("rejected", "bike-99", "rejected"),
            ("no-event", "no-event", "accepted"),
        ] {
            let first = submit(&runtime, id, bicycle).await?;
            assert_eq!(first["status"], "completed", "{first}");
            assert_eq!(first["result"]["decision"], decision, "{first}");
            let winner_calls = calls.load(Ordering::SeqCst);
            let duplicate = submit(&runtime, id, bicycle).await?;
            assert_eq!(duplicate["status"], "completed", "{duplicate}");
            assert_eq!(duplicate["result"]["decision"], decision);
            assert_eq!(
                calls.load(Ordering::SeqCst),
                winner_calls,
                "replay must not reevaluate"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), before + 3);
        assert!(policy.reads.load(Ordering::SeqCst) > policy.executes.load(Ordering::SeqCst));
        assert!(audits.load(Ordering::SeqCst) > 0);
        let history = runtime.store().load(&demo_stream()).await?;
        assert_eq!(
            history.len(),
            3,
            "rejected and no-event winners append no domain events"
        );
        let reads = codecs_used.load(Ordering::SeqCst);
        let diagnostic = runtime
            .tracer_builder()
            .build()?
            .simulate_handler("bike-rental", "rent-bicycle", request("bike-99"))
            .await?;
        assert!(!diagnostic.committed);
        assert!(
            codecs_used.load(Ordering::SeqCst) > reads,
            "diagnostic must use shared codecs"
        );
        assert_eq!(runtime.store().load(&demo_stream()).await?, history);
        policy.revoked.store(true, Ordering::SeqCst);
        let mut responses = connection
            .jetstream()
            .get_stream(
                runtime
                    .config()
                    .messaging
                    .topology()
                    .command_response_stream()
                    .as_str(),
            )
            .await?;
        let raw_responses = responses.info().await?.state.messages;
        assert!(
            raw_responses >= 3,
            "accepted, rejected and no-event raw responses remain retained"
        );
        let denied = submit(&runtime, "accepted", "bike-42").await?;
        assert_eq!(denied["status"], "failed", "{denied}");
        policy.revoked.store(false, Ordering::SeqCst);
        let accepted = submit(&runtime, "accepted", "bike-42").await?;
        let store = runtime.store();
        let key = rostfrei::OutcomeKey::new(
            store.authority().clone(),
            context.name().clone(),
            rostfrei::OperationId::new(
                accepted["operationId"]
                    .as_str()
                    .ok_or("operation identity")?,
            )?,
        );
        let rostfrei::OutcomeLookup::Completed(outcome) = store.lookup_outcome(&key).await? else {
            return Err("missing durable winner".into());
        };
        store
            .expire_details(&key, outcome.marker().revision())
            .await?;
        let expired = submit(&runtime, "accepted", "bike-42").await?;
        assert_eq!(expired["status"], "failed", "{expired}");
        assert_eq!(
            responses.info().await?.state.messages,
            raw_responses,
            "expiry/denial admission must fail despite retained raw business responses"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            before + 4,
            "only diagnostic adds one handler call"
        );
    }
    runtime.stop_workers().await;
    connection.drain().await?;
    Ok(())
}
