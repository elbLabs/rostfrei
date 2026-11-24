#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::redundant_closure_for_method_calls,
    clippy::too_many_lines,
    reason = "integration assertions use bounded test data and retain end-to-end scenarios"
)]

mod support;
rostfrei::install_macro_support!();
use support::{BikeRentalNatsRuntime, RentBicycle};

use std::{
    error::Error,
    fs, io,
    path::PathBuf,
    process,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bike_rental::{
    BicycleRentalStarted, BikeRentalNatsConfig, BikeRentalNatsResourceLimits, FINANCE_CONTEXT,
    RENTAL_CONTEXT, WORKSHOP_CONTEXT,
    demo::{
        demo_fixture, demo_stream, no_rental_fleets_fixture, rented_demo_fixture,
        reservable_branch_fixture, seed_demo,
    },
    payments::{
        CollectionPaymentDecided, CollectionPaymentRequested, PaymentProvider, PaymentResult,
        RentalPaymentChargeReady, RentalReturnAssessed, SandboxPaymentProvider,
    },
    rental_fleet::{
        BicycleRented, BicycleReturned, RegisterBicycle, RentalFleetAggregate,
        ReserveBicycleCategory, ReturnBicycle,
    },
    rental_payment::{CollectionPayment, PaymentId},
    rental_tariff::{ConfigureRentalTariff, RentalTariffAggregate},
    tracer,
    workshop::BicycleMaintenanceRequested,
};
use http_body_util::BodyExt as _;
use rostfrei::messaging::{
    CausationId, CommandAddress, CommandResponse, CommandResponseOutcome, ConsumerConfig,
    CorrelationId, IntegrationEventEnvelope, MessageId, OperationId as MessagingOperationId,
    RetryDelay, SchemaVersion,
};
use rostfrei::nats::{
    CORRELATION_ID_HEADER, IntegrationEventSubject, NatsConnection, NatsConnectionConfig,
    ServerVersion, StreamRetention, connect, provision_durable_consumer,
};
use rostfrei::testing::integration::nats_url as required_nats_url;
use rostfrei::tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandTransportObserver,
    ExpectedMessageKind, ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository,
    NatsApplication, NatsRuntime, ObservedMessageSeries, OperationMode, TestRepository,
    TestScenarioReset, TracerConfig, command_execution_fingerprint,
    http::{self, HttpConfig},
};
use rostfrei::{
    Aggregate, Command, CommandBus, CommandMessageAdapter, DomainEvent, EventHistory,
    IntegrationCommandMapper, IntegrationEvent, IntegrationEventCommandHandler, OperationId,
    OutcomeReader as _, RecordedEvent, command_message_id, command_response_message_id,
    integration_message_id,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    time::Instant,
};
use tower::ServiceExt as _;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const API_TOKEN: &str = "behavioral-integration-capability";

fn context_runtime<'a>(app: &'a NatsApplication, context: &str) -> TestResult<&'a NatsRuntime> {
    app.runtime_for(context)
        .map(Arc::as_ref)
        .ok_or_else(|| io::Error::other(format!("missing runtime for {context}")).into())
}

async fn start_contexts(
    provider: Arc<dyn PaymentProvider>,
    fixture: rostfrei::fixtures::Fixture,
) -> TestResult<(
    NatsConnection,
    tracer::ContextConfigurations,
    NatsApplication,
)> {
    let scope = unique_scope()?;
    let configs = tracer::configurations_with_payments(
        &scope,
        rostfrei::TrafficScope::Test,
        BikeRentalNatsResourceLimits::from_env()?,
        provider,
    )?
    .with_fresh_outcomes()?;
    let connection = connect(&NatsConnectionConfig::new(&scope, required_nats_url()?)).await?;
    let app = configs
        .application_builder(connection.clone())?
        .with_initial_fixture(fixture)
        .provision_and_start()
        .await?;
    Ok((connection, configs, app))
}

async fn stop_contexts(
    connection: &NatsConnection,
    configs: &tracer::ContextConfigurations,
    app: NatsApplication,
) -> TestResult {
    let stopped = app.shutdown().await;
    let cleaned = cleanup_contexts(connection, configs).await;
    let drained = connection.drain().await;
    stopped?;
    cleaned?;
    drained?;
    Ok(())
}

#[derive(Default)]
struct RecordingObserver {
    publications: Mutex<Vec<CommandPublication>>,
}

#[derive(Deserialize)]
struct ConsumerInfo {
    name: String,
    created: String,
    ack_floor: ConsumerSequence,
    num_ack_pending: usize,
    num_pending: u64,
}

#[derive(Deserialize)]
struct ConsumerSequence {
    #[serde(rename = "stream_seq")]
    stream_sequence: u64,
}

#[async_trait]
impl CommandTransportObserver for RecordingObserver {
    async fn command_published(&self, publication: CommandPublication) {
        self.publications.lock().await.push(publication);
    }
}

struct ReturnBicycleAfterRental;

struct GatedSandboxRefund {
    entered: Notify,
    release: Semaphore,
    calls: Mutex<Vec<Value>>,
}

#[async_trait]
impl PaymentProvider for GatedSandboxRefund {
    async fn charge(&self, id: &PaymentId, details: &CollectionPayment) -> PaymentResult {
        SandboxPaymentProvider.charge(id, details).await
    }

    async fn refund_deposit(
        &self,
        id: &PaymentId,
        charge_reference: &str,
        currency: &str,
        amount_minor: u64,
    ) -> PaymentResult {
        self.calls.lock().await.push(json!({"payment_id": id.as_str(), "charge_reference": charge_reference, "currency": currency, "amount_minor": amount_minor}));
        self.entered.notify_one();
        if let Ok(permit) = self.release.acquire().await {
            permit.forget();
        }
        SandboxPaymentProvider
            .refund_deposit(id, charge_reference, currency, amount_minor)
            .await
    }
}

#[tokio::test]
async fn satisfactory_return_automatically_refunds_only_the_deposit_once() -> TestResult {
    let provider = Arc::new(GatedSandboxRefund {
        entered: Notify::new(),
        release: Semaphore::new(0),
        calls: Mutex::new(Vec::new()),
    });
    let (connection, configs, app) = start_contexts(
        provider.clone(),
        bike_rental::demo::booked_city_bike_fixture()?,
    )
    .await?;
    let result: TestResult = async {
        let runtime = context_runtime(&app, RENTAL_CONTEXT)?;
        let finance = context_runtime(&app, FINANCE_CONTEXT)?;
        ensure(configs.contexts().all(|(config, _)| config.commands.iter().all(|route| route.address().name() != "rent-bicycle")), "direct rental must not have an application command consumer")?;
        let catalog = serde_json::to_value(runtime.tracer_builder().build()?.catalog())?;
        ensure(!catalog["contexts"].as_array().into_iter().flatten().flat_map(|context| context["commands"].as_array().into_iter().flatten()).any(|command| command["id"] == "rent-bicycle"), "direct rental must not be in the application catalog")?;
        let collected = runtime.transport().invoke(invocation("collect-for-refund", "refund-flow", "collect-reserved-bicycle", 1, collection_payload("payment-42", "sandbox-approved"))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(collected.outcome(), CommandOutcome::Accepted), "collection failed")?;
        let paid = wait_for_payment(&finance.store(), "payment-42").await?;
        let hired = wait_for_events(&runtime.store(), &demo_stream(), 6).await?;
        ensure(hired.len() == 6 && hired[4].event_type() == "bicycle-rented", "the approved receipt must start the rental before return")?;
        let premature = finance.transport().invoke(invocation("refund-before-return", "refund-flow", "refund-eligible-rental-deposit", 1, json!({"payment_id":"payment-42"}))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(premature.outcome(), CommandOutcome::Rejected(_)) && provider.calls.lock().await.is_empty(), "an unreturned rental must not contact the refund provider")?;
        let request = invocation("return-for-refund", "refund-flow", "return-bicycle", 2, json!({"fleet_id":"city-fleet", "bicycle_id":"bike-42", "returned_at":"2026-12-01T17:00:00Z"}))?;
        let returned = runtime.transport().invoke(request.clone(), Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(returned.outcome(), CommandOutcome::Accepted), "satisfactory return failed")?;
        tokio::time::timeout(Duration::from_secs(10), provider.entered.notified()).await?;
        let history = runtime.store().load(&demo_stream()).await?;
        ensure(history.len() == 8 && history[6].event_type() == "bicycle-returned" && history[7].event_type() == "reserved-bicycle-returned", "physical return and reservation return must both persist before refund confirmation")?;
        ensure_one_operation(&runtime.store(), RENTAL_CONTEXT, &[&history[6], &history[7]]).await?;
        let fleet = rostfrei::CommandExecutor::new(runtime.store()).rehydrate::<RentalFleetAggregate>(&demo_stream()).await?;
        ensure(fleet.state().bicycles()[0].status() == bike_rental::rental_fleet::BicycleStatus::Available, "the bicycle must already be available while the refund is pending")?;
        ensure(finance.store().load(&payment_stream("payment-42")?).await? == paid, "refund must not be recorded before provider confirmation")?;
        ensure(*provider.calls.lock().await == vec![json!({"payment_id":"payment-42", "charge_reference":"sandbox:payment-42", "currency":"EUR", "amount_minor":10000})], "refund must use the original charge and deposit, not the rental charge")?;
        let repeated = runtime.transport().invoke(request, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(repeated.outcome(), CommandOutcome::Accepted), "return retry did not replay")?;
        provider.release.add_permits(1);
        let refunded = wait_for_events(&finance.store(), &payment_stream("payment-42")?, 4).await?;
        ensure(refunded.len() == 4 && refunded[2].event_type() == "rental-return-eligibility-recorded", "Finance must record return eligibility once before its refund")?;
        let payload: Value = serde_json::from_slice(refunded[3].payload())?;
        ensure(refunded[3].event_type() == "security-deposit-refunded" && payload["amount_minor"] == 10000 && payload["currency"] == "EUR", "full deposit refund was not persisted")?;
        let assessment = integration_from::<RentalReturnAssessed>(&connection, &configs, &history[7]).await?;
        ensure(assessment.payload()["eligible"] == true, "on-time undamaged return was not assessed as refundable")?;
        ensure_operation(&finance.store(), FINANCE_CONTEXT, &[&refunded[2], &refunded[3]], Some((assessment.message_id(), "assess-rental-return"))).await?;
        let repeated = finance.transport().invoke(invocation("refund-again", "refund-flow", "refund-eligible-rental-deposit", 1, json!({"payment_id":"payment-42"}))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(repeated.outcome(), CommandOutcome::Accepted) && provider.calls.lock().await.len() == 1, "completed settlement must replay without contacting the provider")?;
        ensure(finance.store().load(&payment_stream("payment-42")?).await? == refunded && runtime.store().load(&demo_stream()).await? == history, "retries duplicated refund or return effects")?;
        ensure_owned_effect(&refunded[3], "finance/rental-payment")?;
        ensure_owned_effect(&history[7], "rental/rental-fleet")?;
        Ok(())
    }.await;
    provider.release.close();
    let cleaned = stop_contexts(&connection, &configs, app).await;
    result?;
    cleaned
}

#[tokio::test]
async fn damaged_return_keeps_the_deposit_while_returning_the_bicycle() -> TestResult {
    Box::pin(return_keeps_deposit(
        "2026-12-01T17:00:00Z",
        Some("Rear brake does not engage"),
    ))
    .await
}

#[tokio::test]
async fn late_return_keeps_the_deposit_while_returning_the_bicycle() -> TestResult {
    Box::pin(return_keeps_deposit("2026-12-01T17:00:01Z", None)).await
}

async fn return_keeps_deposit(returned_at: &str, damage_report: Option<&str>) -> TestResult {
    let provider = Arc::new(GatedSandboxRefund {
        entered: Notify::new(),
        release: Semaphore::new(0),
        calls: Mutex::new(Vec::new()),
    });
    let (connection, configs, app) = start_contexts(
        provider.clone(),
        bike_rental::demo::booked_city_bike_fixture()?,
    )
    .await?;
    let result: TestResult = async {
        let runtime = context_runtime(&app, RENTAL_CONTEXT)?;
        let finance = context_runtime(&app, FINANCE_CONTEXT)?;
        let collected = runtime.transport().invoke(invocation("collect-for-review", "review-flow", "collect-reserved-bicycle", 1, collection_payload("payment-42", "sandbox-approved"))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(collected.outcome(), CommandOutcome::Accepted), "collection for review was not accepted")?;
        let paid = wait_for_payment(&finance.store(), "payment-42").await?;
        let hired = wait_for_events(&runtime.store(), &demo_stream(), 6).await?;
        ensure(hired.len() == 6 && hired[4].event_type() == "bicycle-rented", "the approved receipt must start the rental before return")?;
        let returned = runtime.transport().invoke(invocation("return-for-review", "review-flow", "return-bicycle", 2, json!({"fleet_id":"city-fleet", "bicycle_id":"bike-42", "returned_at":returned_at, "damage_report":damage_report}))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(returned.outcome(), CommandOutcome::Accepted), "review must not prevent recording the physical return")?;
        let history = wait_for_events(&runtime.store(), &demo_stream(), if damage_report.is_some() { 9 } else { 8 }).await?;
        let returned = history.iter().find(|event| event.event_type() == "reserved-bicycle-returned").ok_or("missing reservation return")?;
        let assessment = integration_from::<RentalReturnAssessed>(&connection, &configs, returned).await?;
        let reviewed = wait_for_events(&finance.store(), &payment_stream("payment-42")?, 3).await?;
        ensure(reviewed.len() == 3 && reviewed[..2] == paid && reviewed[2].event_type() == "rental-return-eligibility-recorded" && assessment.payload()["eligible"] == false, "Finance must record that the return is not eligible without refunding")?;
        ensure_operation(&finance.store(), FINANCE_CONTEXT, &[&reviewed[2]], Some((assessment.message_id(), "assess-rental-return"))).await?;
        let settled = finance.transport().invoke(invocation("settle-held-deposit", "review-flow", "refund-eligible-rental-deposit", 1, json!({"payment_id":"payment-42"}))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(settled.outcome(), CommandOutcome::Accepted), "settlement must accept retaining the deposit for review")?;
        ensure(provider.calls.lock().await.is_empty() && finance.store().load(&payment_stream("payment-42")?).await? == reviewed, "a damaged or late return must retain the paid deposit without calling the provider")?;
        let fleet = rostfrei::CommandExecutor::new(runtime.store()).rehydrate::<RentalFleetAggregate>(&demo_stream()).await?;
        ensure(fleet.state().bicycles()[0].status() == bike_rental::rental_fleet::BicycleStatus::Available, "review must not keep the bicycle rented")?;
        ensure(fleet.state().bicycles()[0].condition() == if damage_report.is_some() { bike_rental::rental_fleet::BicycleCondition::MaintenanceRequired } else { bike_rental::rental_fleet::BicycleCondition::Serviceable }, "return lost the bicycle condition")?;
        Ok(())
    }.await;
    provider.release.close();
    let cleaned = stop_contexts(&connection, &configs, app).await;
    result?;
    cleaned
}

struct GatedSandboxPayment {
    entered: Notify,
    release: Semaphore,
    calls: AtomicUsize,
}

#[async_trait]
impl PaymentProvider for GatedSandboxPayment {
    async fn refund_deposit(
        &self,
        id: &PaymentId,
        charge_reference: &str,
        currency: &str,
        amount_minor: u64,
    ) -> PaymentResult {
        SandboxPaymentProvider
            .refund_deposit(id, charge_reference, currency, amount_minor)
            .await
    }
    async fn charge(&self, id: &PaymentId, details: &CollectionPayment) -> PaymentResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        if let Ok(permit) = self.release.acquire().await {
            permit.forget();
        }
        SandboxPaymentProvider.charge(id, details).await
    }
}

fn payment_stream(id: &str) -> TestResult<rostfrei::StreamId> {
    Ok(rostfrei::StreamId::new(
        rostfrei::StreamAggregateType::new("finance/rental-payment")?,
        rostfrei::StreamAggregateId::new(id)?,
    ))
}

fn collection_payload(payment: &str, method: &str) -> Value {
    json!({"fleet_id": "city-fleet", "reservation_id": "booking-42", "bicycle_id": "bike-42", "payment_id": payment, "payment_method": method})
}

async fn ensure_one_operation(
    store: &rostfrei::nats::NatsEventStore,
    context: &str,
    events: &[&RecordedEvent],
) -> TestResult {
    ensure_operation(store, context, events, None).await
}

async fn ensure_operation(
    store: &rostfrei::nats::NatsEventStore,
    context: &str,
    events: &[&RecordedEvent],
    cause: Option<(&MessageId, &str)>,
) -> TestResult {
    let first = events.first().ok_or("operation has no events")?;
    let key = rostfrei::OutcomeKey::new(
        store.authority().clone(),
        rostfrei::messaging::BoundedContextName::new(context)?,
        first.operation_id().clone(),
    );
    let rostfrei::OutcomeLookup::Completed(outcome) = store.lookup_outcome(&key).await? else {
        return Err("operation decision is not retained".into());
    };
    let recorded = outcome.marker().integrity_receipt().events();
    ensure(
        outcome.marker().decision() == rostfrei::core::OutcomeDecision::Accepted
            && recorded.len() == events.len()
            && events.iter().all(|event| recorded.contains(event)),
        "one accepted operation receipt must contain all expected effects",
    )?;
    if let Some((cause, command)) = cause {
        ensure(
            outcome.marker().evidence().is_some_and(|evidence| {
                evidence.original_causation.as_deref() == Some(cause.as_str())
                    && evidence.command_type == command
                    && evidence.events_caused_by_command
            }),
            &format!(
                "{} must be causally linked to integration message {cause}",
                first.event_type()
            ),
        )?;
    }
    Ok(())
}

fn ensure_owned_effect(event: &RecordedEvent, aggregate_type: &str) -> TestResult {
    let actual = event.stream_id().aggregate_type().as_str();
    ensure(
        actual == aggregate_type,
        &format!(
            "{} must persist in {aggregate_type}; recorded in {actual}",
            event.event_type()
        ),
    )
}

async fn wait_for_events(
    store: &impl EventHistory,
    stream: &rostfrei::StreamId,
    expected: usize,
) -> TestResult<Vec<RecordedEvent>> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let events = store.load(stream).await?;
            if events.len() >= expected {
                return TestResult::Ok(events);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| {
        io::Error::other(format!(
            "timed out waiting for {expected} events in {stream:?}"
        ))
    })?
}

async fn wait_for_payment(store: &impl EventHistory, id: &str) -> TestResult<Vec<RecordedEvent>> {
    let events = wait_for_events(store, &payment_stream(id)?, 2).await?;
    ensure(
        events.len() == 2,
        "payment must have one request and one decision",
    )?;
    Ok(events)
}

async fn integration_from<E: IntegrationEvent>(
    connection: &NatsConnection,
    configs: &tracer::ContextConfigurations,
    source: &RecordedEvent,
) -> TestResult<IntegrationEventEnvelope<Value>> {
    let context = configs
        .runtime_config(E::BOUNDED_CONTEXT.ok_or("missing publisher context")?)
        .ok_or("publisher runtime is not configured")?;
    let address = context.context.integration_event_address(E::EVENT_NAME)?;
    let correlation = source
        .correlation_id()
        .ok_or("source event has no correlation")?;
    let subject = IntegrationEventSubject::new(
        address.clone(),
        correlation.clone(),
        Some(CausationId::new(source.event_id().as_str())?),
    )?;
    let expected_id = integration_message_id(
        &address,
        SchemaVersion::new(E::SCHEMA_VERSION)?,
        source.event_id(),
    )?;
    let stream = connection
        .jetstream()
        .get_stream(
            context
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .await?;
    let raw = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match stream.get_last_raw_message_by_subject(subject.as_str()).await {
                Ok(raw) => break Ok(raw),
                Err(error) if error.kind() == async_nats::jetstream::stream::LastRawMessageErrorKind::NoMessageFound => {},
                Err(error) => return Err(error),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.map_err(|_| io::Error::other(format!("timed out waiting for {} caused by {}", E::EVENT_NAME, source.event_id())))??;
    let envelope: IntegrationEventEnvelope<Value> = serde_json::from_slice(&raw.payload)?;
    ensure(
        envelope.message_id() == &expected_id
            && envelope.correlation_id() == correlation
            && envelope.causation_id().map(CausationId::as_str) == Some(source.event_id().as_str()),
        "integration receipt must identify the committed source event and retain its correlation",
    )?;
    Ok(envelope)
}

async fn replay_integration<E: IntegrationEvent>(
    connection: &NatsConnection,
    configs: &tracer::ContextConfigurations,
    app: &NatsApplication,
    destination: &str,
) -> TestResult {
    let config = configs
        .runtime_config(destination)
        .ok_or("destination runtime is not configured")?;
    let consumer = config
        .integrations
        .iter()
        .find(|route| route.address().name() == E::EVENT_NAME)
        .ok_or("integration consumer is not configured")?;
    let runtime = context_runtime(app, destination)?;
    let stream_name = config
        .messaging
        .topology()
        .integration_event_stream()
        .as_str();
    let durable = consumer.durable_name().as_str();
    wait_for_consumer_acknowledgement(connection, stream_name, durable).await?;
    runtime.stop_workers().await;
    // Recreate only the selected subscription, retaining the source messages,
    // owning store and command replay evidence. This forces real broker redelivery.
    let stream = connection.jetstream().get_stream(stream_name).await?;
    ensure(
        stream.delete_consumer(durable).await?.success,
        "broker did not delete the selected integration subscription",
    )?;
    provision_durable_consumer(
        connection.jetstream(),
        config.messaging.topology(),
        consumer,
    )
    .await?;
    runtime.start_workers().await?;
    wait_for_consumer_acknowledgement(connection, stream_name, durable).await
}

#[tokio::test]
async fn collection_waits_for_payment_and_commits_finance_and_rental_once() -> TestResult {
    let provider = Arc::new(GatedSandboxPayment {
        entered: Notify::new(),
        release: Semaphore::new(0),
        calls: AtomicUsize::new(0),
    });
    let (connection, configs, app) = start_contexts(
        provider.clone(),
        bike_rental::demo::booked_city_bike_fixture()?,
    )
    .await?;
    let result: TestResult = async {
        let runtime = context_runtime(&app, RENTAL_CONTEXT)?;
        let finance = context_runtime(&app, FINANCE_CONTEXT)?;
        let receipt = runtime.transport().invoke(invocation("collect", "collection-flow", "collect-reserved-bicycle", 1, collection_payload("payment-42", "sandbox-approved"))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(receipt.outcome(), CommandOutcome::Accepted), "collection was not accepted")?;
        tokio::time::timeout(Duration::from_secs(10), provider.entered.notified()).await?;
        let fleet_events = runtime.store().load(&demo_stream()).await?;
        let payment_events = finance.store().load(&payment_stream("payment-42")?).await?;
        ensure(fleet_events.len() == 4 && fleet_events[3].event_type() == "bicycle-collection-requested" && payment_events.len() == 1,
            "before provider confirmation only the hold and payment request may persist")?;
        ensure_owned_effect(&payment_events[0], "finance/rental-payment")?;
        ensure_owned_effect(&fleet_events[3], "rental/rental-fleet")?;
        ensure_one_operation(&runtime.store(), RENTAL_CONTEXT, &[&fleet_events[3]]).await?;
        let requested = integration_from::<CollectionPaymentRequested>(&connection, &configs, &fleet_events[3]).await?;
        ensure_operation(&finance.store(), FINANCE_CONTEXT, &[&payment_events[0]], Some((requested.message_id(), "record-rental-payment-request"))).await?;
        ensure(runtime.store().authority() != finance.store().authority()
            && runtime.store().load(&payment_stream("payment-42")?).await?.is_empty()
            && finance.store().load(&demo_stream()).await?.is_empty(), "Rental and Finance effects must be physically isolated by context")?;
        ensure(fleet_events[3].correlation_id().is_some() && fleet_events[3].correlation_id() == payment_events[0].correlation_id(), "Finance request must retain the Rental hold's correlation")?;
        let payload: Value = serde_json::from_slice(payment_events[0].payload())?;
        ensure(payload["details"]["quote"] == json!({"currency": "EUR", "rental_charge_minor": 2500, "security_deposit_minor": 10000}), "payment request must preserve rent and refundable deposit separately")?;
        let repeated = runtime.transport().invoke(invocation("repeat-collection-new-request", "collection-flow", "collect-reserved-bicycle", 1, collection_payload("payment-42", "sandbox-approved"))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(repeated.outcome(), CommandOutcome::Accepted), "logical collection retry was not accepted")?;
        let competing = runtime.transport().invoke(invocation("competing-collection", "collection-flow", "collect-reserved-bicycle", 1, collection_payload("competing-payment", "sandbox-approved"))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(competing.outcome(), CommandOutcome::Rejected(_)), "a second payment attempt must not claim a pending reservation")?;
        ensure(finance.store().load(&payment_stream("competing-payment")?).await?.is_empty(), "refused collection must not leave a partial payment request")?;
        ensure(runtime.store().load(&demo_stream()).await? == fleet_events, "pending retries and refusals must preserve the hold")?;
        runtime.stop_workers().await;
        provider.release.add_permits(1);
        let paid = wait_for_payment(&finance.store(), "payment-42").await?;
        let decided = integration_from::<CollectionPaymentDecided>(&connection, &configs, &paid[1]).await?;
        ensure(decided.payload() == &json!({"payment_id":"payment-42", "fleet_id":"city-fleet", "reservation_id":"booking-42", "bicycle_id":"bike-42", "outcome":"approved"}), "Finance receipt omitted the approved collection identity")?;
        ensure(runtime.store().load(&demo_stream()).await? == fleet_events, "Finance can commit and publish approval while Rental is stopped, but must not write Rental state")?;
        runtime.start_workers().await?;
        let final_fleet = wait_for_events(&runtime.store(), &demo_stream(), 6).await?;
        let decision: Value = serde_json::from_slice(paid[1].payload())?;
        ensure(paid[1].event_type() == "rental-payment-approved" && decision["provider_reference"] == "sandbox:payment-42" && decision.get("outcome").is_none(), "provider approval must be an explicit fact")?;
        ensure(final_fleet.len() == 6 && final_fleet[4].event_type() == "bicycle-rented" && final_fleet[5].event_type() == "reserved-bicycle-collected", "approval must start exactly one rental")?;
        ensure_owned_effect(&paid[1], "finance/rental-payment")?;
        let charge_ready = integration_from::<RentalPaymentChargeReady>(&connection, &configs, &paid[0]).await?;
        ensure_operation(&finance.store(), FINANCE_CONTEXT, &[&paid[1]], Some((charge_ready.message_id(), "collect-rental-payment"))).await?;
        ensure_operation(&runtime.store(), RENTAL_CONTEXT, &[&final_fleet[4], &final_fleet[5]], Some((decided.message_id(), "resolve-bicycle-collection"))).await?;
        ensure(paid[1].operation_id() != final_fleet[4].operation_id() && paid[1].correlation_id() == final_fleet[4].correlation_id() && final_fleet[4].causation_id().is_some(), "the Finance decision must cause a separate Rental operation in the same conversation")?;
        let replay = finance.transport().invoke(invocation("repeat-provider-processing", "collection-flow", "collect-rental-payment", 1, json!({"payment_id": "payment-42"}))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(replay.outcome(), CommandOutcome::Accepted), "completed provider processing must replay")?;
        replay_integration::<CollectionPaymentRequested>(&connection, &configs, &app, FINANCE_CONTEXT).await?;
        replay_integration::<RentalPaymentChargeReady>(&connection, &configs, &app, FINANCE_CONTEXT).await?;
        replay_integration::<CollectionPaymentDecided>(&connection, &configs, &app, RENTAL_CONTEXT).await?;
        ensure(provider.calls.load(Ordering::SeqCst) == 1, "logical retries must not contact the provider again")?;
        ensure(runtime.store().load(&demo_stream()).await? == final_fleet && finance.store().load(&payment_stream("payment-42")?).await? == paid, "payment processing retry duplicated durable effects")?;
        let harbor = bike_rental::rental_fleet::stream_id("harbor-fleet")?;
        for (operation, command, version, payload) in [
            ("configure-city-tariff", ConfigureRentalTariff::LOCAL_ID, ConfigureRentalTariff::SCHEMA_VERSION,
                json!({"category":"city", "terms":{"currency":"EUR", "hourly_rate_minor":500, "daily_cap_minor":2500, "security_deposit_minor":10000}})),
            ("add-harbor-bicycle", RegisterBicycle::LOCAL_ID, RegisterBicycle::SCHEMA_VERSION,
                json!({"fleet_id":"harbor-fleet", "bicycle_id":"harbor-42", "condition":"serviceable", "category":"city"})),
            ("book-harbor-bicycle", ReserveBicycleCategory::LOCAL_ID, ReserveBicycleCategory::SCHEMA_VERSION,
                json!({"fleet_id":"harbor-fleet", "reservation_id":"harbor-booking", "customer_id":"customer-sam", "category":"city", "period":{"starts_at":"2026-12-01T09:00:00Z", "ends_at":"2026-12-01T17:00:00Z"}})),
            ("reuse-payment-other-branch", "collect-reserved-bicycle", 1,
                json!({"fleet_id":"harbor-fleet", "reservation_id":"harbor-booking", "bicycle_id":"harbor-42", "payment_id":"payment-42", "payment_method":"sandbox-approved"})),
        ] {
            let receipt = runtime.transport().invoke(invocation(operation, "payment-conflict-flow", command, version, payload)?, Arc::new(RecordingObserver::default())).await?;
            ensure(matches!(receipt.outcome(), CommandOutcome::Accepted), &format!("{operation} was not accepted: {:?}", receipt.outcome()))?;
        }
        let harbor_events = wait_for_events(&runtime.store(), &harbor, 5).await?;
        ensure(harbor_events.len() == 5 && harbor_events[3].event_type() == "bicycle-collection-requested" && harbor_events[4].event_type() == "bicycle-collection-cancelled", "the conflicting branch must get a hold and then release it without renting")?;
        let payment = wait_for_events(&finance.store(), &payment_stream("payment-42")?, 3).await?;
        ensure(payment.len() == 3 && payment[..2] == paid && payment[2].event_type() == "rental-payment-identity-conflict-recorded", "Finance must preserve the original processing and record the identity conflict once")?;
        let requested = integration_from::<CollectionPaymentRequested>(&connection, &configs, &harbor_events[3]).await?;
        ensure_operation(&finance.store(), FINANCE_CONTEXT, &[&payment[2]], Some((requested.message_id(), "record-rental-payment-request"))).await?;
        let conflict = integration_from::<CollectionPaymentDecided>(&connection, &configs, &payment[2]).await?;
        ensure(conflict.payload() == &json!({"payment_id":"payment-42", "fleet_id":"harbor-fleet", "reservation_id":"harbor-booking", "bicycle_id":"harbor-42", "outcome":"declined"}), "Finance must decline the conflicting identity without releasing the original booking")?;
        ensure_operation(&runtime.store(), RENTAL_CONTEXT, &[&harbor_events[4]], Some((conflict.message_id(), "resolve-bicycle-collection"))).await?;
        let rehydrated = rostfrei::CommandExecutor::new(runtime.store()).rehydrate::<RentalFleetAggregate>(&harbor).await?;
        ensure(rehydrated.state().bicycles()[0].status() == bike_rental::rental_fleet::BicycleStatus::Available && rehydrated.state().reservations()[0].collection().is_none(), "the other branch's rejected attempt must free its bike but preserve its reservation")?;
        ensure(runtime.store().load(&demo_stream()).await? == final_fleet && provider.calls.load(Ordering::SeqCst) == 1, "payment ID reuse changed the original rental or called the provider again")?;
        Ok(())
    }.await;
    provider.release.close();
    let cleaned = stop_contexts(&connection, &configs, app).await;
    result?;
    cleaned
}

#[tokio::test]
async fn declined_payment_releases_the_bicycle_and_keeps_the_booking_for_a_new_attempt()
-> TestResult {
    let (connection, configs, app) = start_contexts(
        Arc::new(SandboxPaymentProvider),
        bike_rental::demo::booked_city_bike_fixture()?,
    )
    .await?;
    let result: TestResult = async {
        let runtime = context_runtime(&app, RENTAL_CONTEXT)?;
        let finance = context_runtime(&app, FINANCE_CONTEXT)?;
        let declined = runtime
            .transport()
            .invoke(
                invocation(
                    "declined-collection",
                    "decline-flow",
                    "collect-reserved-bicycle",
                    1,
                    collection_payload("declined-payment", "sandbox-declined"),
                )?,
                Arc::new(RecordingObserver::default()),
            )
            .await?;
        ensure(
            matches!(declined.outcome(), CommandOutcome::Accepted),
            "declining attempt was not requested",
        )?;
        let denied = wait_for_payment(&finance.store(), "declined-payment").await?;
        let denied_payload: Value = serde_json::from_slice(denied[1].payload())?;
        ensure(
            denied[1].event_type() == "rental-payment-declined"
                && denied_payload.get("outcome").is_none(),
            "sandbox decline was not recorded",
        )?;
        let events = wait_for_events(&runtime.store(), &demo_stream(), 5).await?;
        ensure(
            events.len() == 5 && events[4].event_type() == "bicycle-collection-cancelled",
            "declined payment must release the hold without renting",
        )?;
        ensure_owned_effect(&denied[1], "finance/rental-payment")?;
        ensure_owned_effect(&events[4], "rental/rental-fleet")?;
        ensure_one_operation(&finance.store(), FINANCE_CONTEXT, &[&denied[1]]).await?;
        let decided =
            integration_from::<CollectionPaymentDecided>(&connection, &configs, &denied[1]).await?;
        ensure(
            decided.payload()["outcome"] == "declined"
                && decided.payload()["payment_id"] == "declined-payment",
            "decline receipt must identify its failed payment",
        )?;
        ensure_operation(
            &runtime.store(),
            RENTAL_CONTEXT,
            &[&events[4]],
            Some((decided.message_id(), "resolve-bicycle-collection")),
        )
        .await?;
        ensure(
            denied[1].operation_id() != events[4].operation_id()
                && denied[1].correlation_id().is_some()
                && denied[1].correlation_id() == events[4].correlation_id()
                && events[4].causation_id().is_some(),
            "the Finance decline must cause a separate Rental release in the same conversation",
        )?;
        let fleet = rostfrei::CommandExecutor::new(runtime.store())
            .rehydrate::<RentalFleetAggregate>(&demo_stream())
            .await?;
        ensure(
            fleet.state().bicycles()[0].status()
                == bike_rental::rental_fleet::BicycleStatus::Available
                && fleet.state().reservations().len() == 1
                && fleet.state().reservations()[0].collection().is_none(),
            "decline must retain the confirmed reservation and release its bicycle",
        )?;
        let next = runtime
            .transport()
            .invoke(
                invocation(
                    "new-payment-attempt",
                    "decline-flow",
                    "collect-reserved-bicycle",
                    1,
                    collection_payload("approved-payment", "sandbox-approved"),
                )?,
                Arc::new(RecordingObserver::default()),
            )
            .await?;
        ensure(
            matches!(next.outcome(), CommandOutcome::Accepted),
            "a new payment attempt must remain possible",
        )?;
        let paid = wait_for_payment(&finance.store(), "approved-payment").await?;
        let final_events = wait_for_events(&runtime.store(), &demo_stream(), 8).await?;
        ensure(
            final_events.len() == 8
                && final_events[6].event_type() == "bicycle-rented"
                && final_events[7].event_type() == "reserved-bicycle-collected",
            "approval on the new attempt must start one rental",
        )?;
        let replay = finance
            .transport()
            .invoke(
                invocation(
                    "old-decline-replayed",
                    "decline-flow",
                    "collect-rental-payment",
                    1,
                    json!({"payment_id":"declined-payment"}),
                )?,
                Arc::new(RecordingObserver::default()),
            )
            .await?;
        ensure(
            matches!(replay.outcome(), CommandOutcome::Accepted),
            "old decline must replay without a new charge",
        )?;
        replay_integration::<RentalPaymentChargeReady>(
            &connection,
            &configs,
            &app,
            FINANCE_CONTEXT,
        )
        .await?;
        replay_integration::<CollectionPaymentDecided>(&connection, &configs, &app, RENTAL_CONTEXT)
            .await?;
        ensure(
            runtime.store().load(&demo_stream()).await? == final_events
                && finance
                    .store()
                    .load(&payment_stream("approved-payment")?)
                    .await?
                    == paid
                && finance
                    .store()
                    .load(&payment_stream("declined-payment")?)
                    .await?
                    == denied,
            "old decline must not release a later successful rental",
        )?;
        Ok(())
    }
    .await;
    let cleaned = stop_contexts(&connection, &configs, app).await;
    result?;
    cleaned
}

impl IntegrationCommandMapper<BicycleRentalStarted> for ReturnBicycleAfterRental {
    type Command = ReturnBicycle;
    type Error = &'static str;

    fn map(&self, event: &BicycleRentalStarted) -> Result<Self::Command, Self::Error> {
        Ok(ReturnBicycle {
            fleet_id: event.fleet_id().clone(),
            bicycle_id: event.bicycle_id().clone(),
            damage_report: None,
            returned_at: None,
        })
    }
}

#[tokio::test]
async fn reservation_quotes_remain_fixed_after_tariff_changes() -> TestResult {
    let scope = unique_scope()?;
    let config = BikeRentalNatsConfig::new_test_with_resource_limits(
        &scope,
        BikeRentalNatsResourceLimits::from_env()?,
    )?
    .with_fresh_outcomes()?;
    let connection = connect(&NatsConnectionConfig::new(&scope, required_nats_url()?)).await?;
    let runtime =
        BikeRentalNatsRuntime::provision_config(connection.clone(), config.clone()).await?;
    let result: TestResult = async {
        runtime.apply_fixture(&reservable_branch_fixture()?).await?;
        runtime.start_workers().await?;
        let booking = json!({"fleet_id": "city-fleet", "reservation_id": "quoted-booking", "customer_id": "customer-alex", "category": "city",
            "period": {"starts_at": "2026-12-01T09:00:00Z", "ends_at": "2026-12-01T17:00:00Z"}});
        let receipt = runtime.transport().invoke(invocation("original-quote", "quote-flow", "reserve-bicycle-category", 1, booking.clone())?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(receipt.outcome(), CommandOutcome::Accepted), "initial quote was not accepted")?;
        let original = runtime.store().load(&demo_stream()).await?;
        let original_event = original.last().ok_or("missing original booking")?;
        let payload: Value = serde_json::from_slice(original_event.payload())?;
        ensure(payload["quote"] == json!({"currency": "EUR", "rental_charge_minor": 2500, "security_deposit_minor": 10000}), "reservation did not snapshot its capped rental and deposit")?;
        let changed = runtime.transport().invoke(invocation("new-tariff", "quote-flow", "configure-rental-tariff", 1,
            json!({"category": "city", "terms": {"currency": "EUR", "hourly_rate_minor": 1000, "daily_cap_minor": 5000, "security_deposit_minor": 20000}}))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(changed.outcome(), CommandOutcome::Accepted), "tariff update was not accepted")?;
        let tariff_stream = rostfrei::StreamId::new(
            rostfrei::StreamAggregateType::new(RentalTariffAggregate::aggregate_type().into_owned())?,
            rostfrei::StreamAggregateId::new("city")?,
        );
        let tariff = runtime.store().load(&tariff_stream).await?;
        ensure(tariff.len() == 2 && tariff[1].event_type() == "rental-tariff-configured", "tariff change must persist as a separate Rental aggregate effect")?;
        ensure_owned_effect(&tariff[1], "rental/rental-tariff")?;
        ensure_owned_effect(original_event, "rental/rental-fleet")?;
        let replay = runtime.transport().invoke(invocation("same-booking-new-request", "quote-flow", "reserve-bicycle-category", 1, booking)?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(replay.outcome(), CommandOutcome::Accepted), "a tariff update must not turn the same booking into a conflict")?;
        ensure(runtime.store().load(&demo_stream()).await? == original, "same booking must preserve the original recorded quote")?;
        let next = runtime.transport().invoke(invocation("new-booking-new-tariff", "quote-flow", "reserve-bicycle-category", 1,
            json!({"fleet_id": "city-fleet", "reservation_id": "tomorrow-booking", "customer_id": "customer-sam", "category": "city",
                "period": {"starts_at": "2026-12-02T09:00:00Z", "ends_at": "2026-12-02T17:00:00Z"}}))?, Arc::new(RecordingObserver::default())).await?;
        ensure(matches!(next.outcome(), CommandOutcome::Accepted), "new booking was not accepted")?;
        let history = runtime.store().load(&demo_stream()).await?;
        let payload: Value = serde_json::from_slice(history.last().ok_or("missing new booking")?.payload())?;
        ensure(history.len() == 6 && payload["quote"] == json!({"currency": "EUR", "rental_charge_minor": 5000, "security_deposit_minor": 20000}), "new bookings must use the changed rental rate and deposit")?;
        Ok(())
    }.await;
    runtime.stop_workers().await;
    let cleaned = cleanup(&connection, [&config]).await;
    let drained = connection.drain().await;
    result?;
    cleaned?;
    drained?;
    Ok(())
}

#[tokio::test]
async fn reservations_hold_only_serviceable_category_capacity_and_replay_without_charging()
-> TestResult {
    let scope = unique_scope()?;
    let config = BikeRentalNatsConfig::new_test_with_resource_limits(
        &scope,
        BikeRentalNatsResourceLimits::from_env()?,
    )?
    .with_fresh_outcomes()?;
    let connection = connect(&NatsConnectionConfig::new(&scope, required_nats_url()?)).await?;
    let runtime =
        BikeRentalNatsRuntime::provision_config(connection.clone(), config.clone()).await?;
    let result: TestResult = async {
        runtime.start_workers().await?;
        for (id, condition) in [("bike-42", "serviceable"), ("bike-99", "maintenance-required")] {
            let receipt = runtime.transport().invoke(
                invocation(id, "prepare-reservable-branch", RegisterBicycle::LOCAL_ID, RegisterBicycle::SCHEMA_VERSION,
                    json!({"fleet_id": "city-fleet", "bicycle_id": id, "condition": condition, "category": "city"}))?,
                Arc::new(RecordingObserver::default()),
            ).await?;
            ensure(matches!(receipt.outcome(), CommandOutcome::Accepted), "category inventory was not added")?;
        }
        let payload = |id: &str, start: &str, end: &str| json!({
            "fleet_id": "city-fleet", "reservation_id": id, "customer_id": "customer-alex", "category": "city",
            "period": {"starts_at": start, "ends_at": end}
        });
        let unpriced = runtime.transport().invoke(
            invocation("without-tariff", "reservation-flow", ReserveBicycleCategory::LOCAL_ID, 1,
                payload("unpriced-booking", "2026-12-01T09:00:00Z", "2026-12-01T17:00:00Z"))?,
            Arc::new(RecordingObserver::default()),
        ).await?;
        ensure(matches!(unpriced.outcome(), CommandOutcome::Rejected(rejection) if rejection.code == "RESERVATION_REJECTED"
            && rejection.details.as_ref().is_some_and(|details| details["reason"] == "quote-unavailable")),
            "a new booking must be refused when its category has no tariff")?;
        ensure(runtime.store().load(&demo_stream()).await?.len() == 4, "an unpriced booking must not reserve capacity")?;
        let configured = runtime.transport().invoke(
            invocation("city-tariff", "prepare-reservable-branch", ConfigureRentalTariff::LOCAL_ID, ConfigureRentalTariff::SCHEMA_VERSION,
                json!({"category": "city", "terms": {"currency": "EUR", "hourly_rate_minor": 500, "daily_cap_minor": 2500, "security_deposit_minor": 10000}}))?,
            Arc::new(RecordingObserver::default()),
        ).await?;
        ensure(matches!(configured.outcome(), CommandOutcome::Accepted), "city tariff was not configured")?;
        let first = payload("booking-42", "2026-12-01T09:00:00Z", "2026-12-01T17:00:00Z");
        for _ in 0..2 {
            let receipt = runtime.transport().invoke(
                invocation("reserve-city-bike", "reservation-flow", ReserveBicycleCategory::LOCAL_ID, 1, first.clone())?,
                Arc::new(RecordingObserver::default()),
            ).await?;
            ensure(matches!(receipt.outcome(), CommandOutcome::Accepted), "reservation and retry must be accepted")?;
        }
        let history = runtime.store().load(&demo_stream()).await?;
        ensure(history.len() == 5 && history[4].event_type() == "bicycle-reservation-confirmed",
            "booking must persist once after categorized inventory, without a rental or payment")?;
        let confirmed: Value = serde_json::from_slice(history[4].payload())?;
        ensure(confirmed["reservation_id"] == "booking-42" && confirmed["customer_id"] == "customer-alex"
            && confirmed["category"] == "city" && confirmed["period"] == first["period"],
            "confirmation must retain customer, category and requested period")?;
        let overlap = runtime.transport().invoke(
            invocation("overlapping-booking", "reservation-flow", ReserveBicycleCategory::LOCAL_ID, 1,
                payload("booking-43", "2026-12-01T12:00:00Z", "2026-12-01T18:00:00Z"))?,
            Arc::new(RecordingObserver::default()),
        ).await?;
        ensure(matches!(overlap.outcome(), CommandOutcome::Rejected(rejection) if rejection.code == "RESERVATION_REJECTED"
            && rejection.details.as_ref().is_some_and(|details| details["reason"] == "category-unavailable")),
            "overlap must be refused; a bike awaiting repair is not bookable capacity")?;
        ensure(runtime.store().load(&demo_stream()).await?.len() == 5, "refused overlap must not append events")?;
        let adjacent = runtime.transport().invoke(
            invocation("adjacent-booking", "reservation-flow", ReserveBicycleCategory::LOCAL_ID, 1,
                payload("booking-44", "2026-12-01T17:00:00Z", "2026-12-01T18:00:00Z"))?,
            Arc::new(RecordingObserver::default()),
        ).await?;
        ensure(matches!(adjacent.outcome(), CommandOutcome::Accepted), "the next time window must remain bookable")?;
        ensure(runtime.store().load(&demo_stream()).await?.len() == 6, "adjacent booking must append exactly once")?;
        Ok(())
    }.await;
    runtime.stop_workers().await;
    let cleaned = cleanup(&connection, [&config]).await;
    let drained = connection.drain().await;
    result?;
    cleaned?;
    drained?;
    Ok(())
}

#[tokio::test]
async fn damaged_return_blocks_hire_and_opens_one_repair_job() -> TestResult {
    let scope = unique_scope()?;
    let configs = tracer::configurations(
        &scope,
        rostfrei::TrafficScope::Test,
        BikeRentalNatsResourceLimits::from_env()?,
    )?
    .with_fresh_outcomes()?;
    let connection = connect(&NatsConnectionConfig::new(&scope, required_nats_url()?)).await?;
    let app = support::application_with_direct_command(&configs, connection.clone())?
        .with_initial_fixture(rented_demo_fixture()?)
        .provision_and_start()
        .await?;
    let result: TestResult = async {
        let runtime = context_runtime(&app, RENTAL_CONTEXT)?;
        let workshop = context_runtime(&app, WORKSHOP_CONTEXT)?;
        let payload = json!({
            "fleet_id": "city-fleet",
            "bicycle_id": "bike-42",
            "damage_report": "Rear brake does not engage"
        });
        for _ in 0..2 {
            let receipt = runtime
                .transport()
                .invoke(
                    invocation(
                        "damaged-return",
                        "damaged-return-flow",
                        ReturnBicycle::LOCAL_ID,
                        ReturnBicycle::SCHEMA_VERSION,
                        payload.clone(),
                    )?,
                    Arc::new(RecordingObserver::default()),
                )
                .await?;
            ensure(
                matches!(receipt.outcome(), CommandOutcome::Accepted),
                "damaged return and its exact retry must be accepted",
            )?;
        }
        let history = runtime.store().load(&demo_stream()).await?;
        ensure(
            history.len() == 5
                && history[3].event_type() == "bicycle-returned"
                && history[4].event_type() == "bicycle-damage-reported",
            "damaged return must commit one return and one damage report",
        )?;
        let damage = &history[4];
        ensure(
            history[3].commit_id() == damage.commit_id(),
            "return and maintenance condition must be committed atomically",
        )?;
        let repair_stream = rostfrei::StreamId::new(
            rostfrei::StreamAggregateType::new("workshop/repair-job")?,
            rostfrei::StreamAggregateId::new(damage.event_id().as_str())?,
        );
        let repair_history = wait_for_events(&workshop.store(), &repair_stream, 1).await?;
        ensure(workshop.store().authority() != runtime.store().authority()
            && workshop.store().load(&demo_stream()).await?.is_empty()
            && runtime.store().load(&repair_stream).await?.is_empty(), "Workshop repair and Rental damage must stay in separate stores")?;
        ensure(
            repair_history.len() == 1 && repair_history[0].event_type() == "repair-job-opened",
            "workshop must persist exactly one repair job",
        )?;
        ensure_owned_effect(&repair_history[0], "workshop/repair-job")?;
        ensure_owned_effect(damage, "rental/rental-fleet")?;
        ensure_one_operation(&runtime.store(), RENTAL_CONTEXT, &[&history[3], damage]).await?;
        let repair: Value = serde_json::from_slice(repair_history[0].payload())?;
        ensure(
            repair["fleet_id"] == "city-fleet"
                && repair["bicycle_id"] == "bike-42"
                && repair["damage_report"] == "Rear brake does not engage",
            "workshop must retain the bike, station and damage report",
        )?;
        ensure(
            repair_history[0].correlation_id() == damage.correlation_id(),
            "workshop handoff must preserve the return correlation",
        )?;
        let notification = integration_from::<BicycleMaintenanceRequested>(&connection, &configs, damage).await?;
        ensure(
            notification.causation_id().map(CausationId::as_str) == Some(damage.event_id().as_str())
                && notification.payload()["source_event_id"] == damage.event_id().as_str()
                && notification.payload()["bicycle_id"] == "bike-42"
                && notification.payload()["damage_report"] == "Rear brake does not engage",
            "workshop notification must carry the recorded damage and its explicit cause",
        )?;
        ensure_operation(
            &workshop.store(),
            WORKSHOP_CONTEXT,
            &[&repair_history[0]],
            Some((notification.message_id(), "open-repair-job")),
        )
        .await?;
        replay_integration::<BicycleMaintenanceRequested>(&connection, &configs, &app, WORKSHOP_CONTEXT).await?;
        ensure(workshop.store().load(&repair_stream).await? == repair_history, "return retry and notification redelivery must not duplicate the repair job")?;
        let receipt = runtime
            .transport()
            .invoke(
                invocation(
                    "hire-after-damaged-return",
                    "hire-after-damaged-return",
                    RentBicycle::LOCAL_ID,
                    RentBicycle::SCHEMA_VERSION,
                    json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
                )?,
                Arc::new(RecordingObserver::default()),
            )
            .await?;
        ensure(
            matches!(receipt.outcome(), CommandOutcome::Rejected(rejection) if rejection.code == "BICYCLE_UNAVAILABLE"),
            "a returned damaged bicycle must remain unavailable for hire",
        )?;
        ensure(
            runtime.store().load(&demo_stream()).await?.len() == 5,
            "refused hire must not append a rental event",
        )?;
        Ok(())
    }
    .await;
    let cleaned = stop_contexts(&connection, &configs, app).await;
    result?;
    cleaned
}

#[tokio::test]
async fn command_workers_and_test_reset_are_subject_scope_isolated() -> TestResult {
    let nats_url = required_nats_url()?;
    let scope = unique_scope()?;
    let resource_limits = BikeRentalNatsResourceLimits::from_env()?;
    let test_config = BikeRentalNatsConfig::new_test_with_resource_limits(&scope, resource_limits)?
        .with_fresh_completed_outcome_epoch("isolated-runtime-fixture")?;
    let production_config =
        BikeRentalNatsConfig::new_with_resource_limits(&scope, resource_limits)?
            .with_fresh_completed_outcome_epoch("isolated-runtime-fixture")?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{scope}-integration"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let result = async {
        let test_runtime = Arc::new(
            BikeRentalNatsRuntime::provision_config(connection.clone(), test_config.clone())
                .await?,
        );
        let production_runtime = Arc::new(
            BikeRentalNatsRuntime::provision_config(connection.clone(), production_config.clone())
                .await?,
        );
        let fixture = demo_fixture()?;
        test_runtime.apply_fixture(&fixture).await?;
        seed_demo(&production_runtime.store()).await?;
        test_runtime.start_workers().await?;
        production_runtime.start_workers().await?;

        let result = run_isolation_test(&connection, &test_runtime, &production_runtime).await;
        test_runtime.stop_workers().await;
        production_runtime.stop_workers().await;
        result
    }
    .await;
    let cleanup = cleanup(&connection, [&test_config, &production_config]).await;
    let drain = connection.drain().await;

    result?;
    cleanup?;
    drain?;
    Ok(())
}

#[tokio::test]
async fn behavioral_definitions_pass_through_http_and_the_isolated_nats_runtime() -> TestResult {
    let nats_url = required_nats_url()?;
    let scope = unique_scope()?;
    let resource_limits = BikeRentalNatsResourceLimits::from_env()?;
    let test_configs =
        tracer::configurations(&scope, rostfrei::TrafficScope::Test, resource_limits)?
            .with_fresh_completed_outcome_epoch("isolated-runtime-fixture")?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{scope}-behavioral-integration"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let result: TestResult = async {
        let test_repository: Arc<dyn TestRepository> = Arc::new(FilesystemTestRepository::load(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tracer"),
        )?);
        let app_config = TracerConfig::new(tokio::net::TcpListener::bind("127.0.0.1:0").await?, HttpConfig::new(API_TOKEN)?)
            .with_test_repository(test_repository)
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
        let host = support::application_with_direct_command(&test_configs, connection.clone())?
            .with_tracer(Some(app_config))
            .with_default_test_fixture(demo_fixture()?)
            .with_test_fixture(no_rental_fleets_fixture()?)
            .with_test_fixture(rented_demo_fixture()?)
            .with_test_fixture(reservable_branch_fixture()?)
            .with_test_fixture(bike_rental::demo::booked_city_bike_fixture()?)
            .with_test_fixture(bike_rental::demo::requested_rental_payment_fixture()?)
            .with_test_fixture(bike_rental::demo::returned_city_bike_fixture()?)
            .provision_and_start().await?;

        let run_result: TestResult = async {
            let test_runtime = context_runtime(&host, RENTAL_CONTEXT)?;
            let test_config = test_configs.rental();
            let app = http::router(host.tracer().ok_or("host omitted Tracer")?.clone(), HttpConfig::new(API_TOKEN)?);
            let definition_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tracer");
            let response = app
                .clone()
                .oneshot(
                    authorize(Request::builder())
                        .uri("/tests")
                        .body(Body::empty())?,
                )
                .await?;
            ensure(
                response.status() == StatusCode::OK,
                "behavioral test discovery did not succeed",
            )?;
            let definitions = json_response(response).await?;
            let definitions = definitions["items"].as_array().ok_or_else(|| {
                io::Error::other("behavioral test discovery returned invalid JSON")
            })?;
            ensure(
                definitions.len() == 7,
                "expected exactly seven bike-rental behavioral definitions",
            )?;

            let canonical_bytes = fs::read(definition_root.join("../support/definitions/rent-available-bicycle.json"))?;
            let canonical_definition: Value = serde_json::from_slice(&canonical_bytes)?;
            let response = app
                .clone()
                .oneshot(
                    authorize(Request::builder())
                        .method("POST")
                        .uri("/test-runs")
                        .header("content-type", "application/json")
                        .body(Body::from(canonical_bytes))?,
                )
                .await?;
            ensure(
                response.status() == StatusCode::OK,
                "inline behavioral test execution did not return a typed report",
            )?;
            let report = json_response(response).await?;
            ensure(
                report["status"] == "passed",
                "inline behavioral test failed",
            )?;
            ensure(
                report["testId"] == "rent-available-bicycle",
                "inline report used the wrong test ID",
            )?;
            ensure(
                report.get("revision").is_none(),
                "inline report unexpectedly included a repository revision",
            )?;
            ensure(
                report["expected"] == canonical_definition["expected"],
                "inline report changed the submitted expectation",
            )?;
            ensure(
                report["comparison"]["status"] == "passed"
                    && report["comparison"]["diagnostics"] == json!([]),
                "server-side message-series comparison did not pass cleanly",
            )?;

            let operation_id = required_json_str(&report, "/operationId")?;
            let correlation_id = required_json_str(&report, "/correlationId")?;
            let command_context = RENTAL_CONTEXT;
            let aggregate_type = RentalFleetAggregate::aggregate_type().into_owned();
            let command_payload = json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"});
            let expected_command_message_id = command_message_id(
                &test_runtime
                    .config()
                    .context.command_address(RentBicycle::LOCAL_ID)?,
                &MessagingOperationId::new(operation_id)?,
                command_execution_fingerprint(
                    command_context,
                    RentBicycle::LOCAL_ID,
                    RentBicycle::SCHEMA_VERSION,
                    &command_payload,
                ),
                &CorrelationId::new(correlation_id)?,
                None,
            )?;
            let expected_response_message_id =
                command_response_message_id(&expected_command_message_id)?;

            ensure(
                report["operationHref"] == format!("/operations/{operation_id}"),
                "inline report operation link is invalid",
            )?;
            ensure(
                report["operationEventsHref"] == format!("/operations/{operation_id}/events"),
                "inline report operation-events link is invalid",
            )?;
            ensure(
                report["correlationEventsHref"] == format!("/correlations/{correlation_id}/events"),
                "inline report correlation-events link is invalid",
            )?;
            ensure(
                report["operation"]["operationId"] == operation_id
                    && report["operation"]["correlationId"] == correlation_id
                    && report["operation"]["mode"] == "test"
                    && report["operation"]["status"] == "completed"
                    && report["operation"]["command"] == RentBicycle::LOCAL_ID
                    && report["operation"]["schemaVersion"] == RentBicycle::SCHEMA_VERSION
                    && report["operation"]["context"] == command_context,
                "inline report operation snapshot is not the executed Test command",
            )?;
            ensure(
                report["operation"]["result"]["decision"] == "accepted"
                    && report["operation"]["result"].get("appended").is_none()
                    && report["operation"]["result"]["published"] == true
                    && report["operation"]["result"]["duplicate"] == false
                    && report["operation"]["result"]["commandMessageId"]
                        == expected_command_message_id.as_str()
                    && report["operation"]["result"]["responseMessageId"]
                        == expected_response_message_id.as_str(),
                "operation result did not retain the exact durable command identities",
            )?;

            let observed: ObservedMessageSeries =
                serde_json::from_value(report["observed"].clone())?;
            ensure(
                observed.messages().len() == 3 && observed.command_outcomes().len() == 1,
                "observed series is not the complete command/event/outcome chain",
            )?;
            ensure(
                observed.topology_issues().is_empty() && observed.outcome_issues().is_empty(),
                "observed series contains unresolved causal or outcome links",
            )?;
            ensure(
                observed
                    .messages()
                    .iter()
                    .all(|message| message.correlation_id() == correlation_id)
                    && observed
                        .command_outcomes()
                        .iter()
                        .all(|outcome| outcome.correlation_id() == correlation_id),
                "observed series did not preserve the report correlation",
            )?;
            let mut observation_orders = observed
                .messages()
                .iter()
                .map(rostfrei::tracer::ObservedMessageNode::observation_order)
                .chain(
                    observed
                        .command_outcomes()
                        .iter()
                        .map(rostfrei::tracer::ObservedCommandOutcome::observation_order),
                )
                .collect::<Vec<_>>();
            observation_orders.sort_unstable();
            ensure(
                observation_orders == [0, 1, 2, 3],
                "observationOrder values are not complete and unique",
            )?;

            let command = observed_message(&observed, ExpectedMessageKind::Command)?;
            let domain_event = observed_message(&observed, ExpectedMessageKind::DomainEvent)?;
            let integration_event =
                observed_message(&observed, ExpectedMessageKind::IntegrationEvent)?;
            ensure(
                command.message_id() == expected_command_message_id.as_str()
                    && command.causation_id().is_none()
                    && command.name() == RentBicycle::LOCAL_ID
                    && command.schema_version() == RentBicycle::SCHEMA_VERSION
                    && command.payload() == Some(&command_payload)
                    && command.context() == Some(command_context),
                "observed root command identity or content is invalid",
            )?;

            let command_outcome = observed
                .command_outcomes()
                .first()
                .ok_or_else(|| io::Error::other("observed command outcome is absent"))?;
            ensure(
                command_outcome.command_message_id() == expected_command_message_id.as_str()
                    && command_outcome.response_message_id()
                        == expected_response_message_id.as_str()
                    && serde_json::to_value(command_outcome.outcome())?
                        == json!({"status": "accepted"})
                    && report["commandOutcome"] == serde_json::to_value(command_outcome)?,
                "durable command response is absent or linked to the wrong command",
            )?;

            let history = wait_for_events(&test_runtime.store(), &demo_stream(), 3).await?;
            ensure(history.len() == 3, "rental history contained extra effects")?;
            for (index, bicycle_id, condition, correlation_id) in [
                (0, "bike-42", "serviceable", "fixture:demo-fleet:3:bike-42"),
                (
                    1,
                    "bike-99",
                    "maintenance-required",
                    "fixture:demo-fleet:3:bike-99",
                ),
            ] {
                let fixture_event = history
                    .get(index)
                    .ok_or_else(|| io::Error::other("demo fixture domain event is absent"))?;
                let fixture_payload: Value = serde_json::from_slice(fixture_event.payload())?;
                ensure(
                    fixture_event.event_type() == "bicycle-added"
                        && fixture_event
                            .operation_id()
                            .as_str()
                            .starts_with("fixture:")
                        && fixture_event.stream_version().value()
                            == u64::try_from(index)?.saturating_add(1)
                        && fixture_event
                            .correlation_id()
                            .is_some_and(|id| id.as_str() == correlation_id)
                        && fixture_event.causation_id().is_none()
                        && fixture_payload
                            == json!({
                                "fleet_id": "city-fleet",
                                "bicycle_id": bicycle_id,
                                "condition": condition
                            }),
                    "inline run did not apply the deterministic demo MessageSeries fixture",
                )?;
            }
            let persisted_event = history
                .get(2)
                .ok_or_else(|| io::Error::other("rental event was not persisted"))?;
            let persisted_payload: Value = serde_json::from_slice(persisted_event.payload())?;
            ensure(
                persisted_event.event_type() == "bicycle-rented"
                    && persisted_event.schema_version() == 1
                    && persisted_event.operation_id().as_str() == operation_id
                    && persisted_event
                        .correlation_id()
                        .is_some_and(|id| id.as_str() == correlation_id)
                    && persisted_event.causation_id().map(CausationId::as_str)
                        == Some(expected_command_message_id.as_str())
                    && persisted_event.stream_id().aggregate_type().as_str() == aggregate_type
                    && persisted_event.stream_id().aggregate_id().as_str() == "city-fleet"
                    && persisted_payload
                        == json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
                "persisted domain event does not prove the executed command chain",
            )?;
            ensure(
                domain_event.message_id() == persisted_event.event_id().as_str()
                    && domain_event.causation_id() == Some(expected_command_message_id.as_str())
                    && domain_event.name() == persisted_event.event_type()
                    && domain_event.schema_version() == persisted_event.schema_version()
                    && domain_event.payload() == Some(&persisted_payload)
                    && domain_event.aggregate().is_some_and(|aggregate| {
                        aggregate.aggregate_type == aggregate_type && aggregate.id == "city-fleet"
                    }),
                "observed domain event is not the actual persisted event",
            )?;

            let exact_integration_message_id = wait_for_integration_chain(
                &connection,
                test_config,
                persisted_event,
                expected_command_message_id.as_str(),
                1,
            )
            .await?;
            ensure(
                integration_event.message_id() == exact_integration_message_id
                    && integration_event.causation_id()
                        == Some(persisted_event.event_id().as_str())
                    && integration_event.name() == "bicycle-rental-started"
                    && integration_event.schema_version() == 1
                    && integration_event.payload()
                        == Some(&json!({
                            "source_event_id": persisted_event.event_id().as_str(),
                            "fleet_id": "city-fleet",
                            "bicycle_id": "bike-42"
                        })),
                "observed integration event is not the exact published event",
            )?;
            wait_for_command_stream_empty(&connection, test_config, [RENTAL_CONTEXT, FINANCE_CONTEXT, WORKSHOP_CONTEXT]
                .into_iter().map(|context| context_runtime(&host, context)).collect::<TestResult<Vec<_>>>()?
                .into_iter().flat_map(|runtime| runtime.config().commands.iter()).collect()).await?;

            let comparison_matches = report["comparison"]["matches"]
                .as_array()
                .ok_or_else(|| io::Error::other("comparison matches are not an array"))?;
            ensure(
                comparison_matches.len() == 3
                    && comparison_matches.iter().any(|matched| {
                        matched["expectedKey"] == "subject"
                            && matched["observedMessageId"] == expected_command_message_id.as_str()
                    })
                    && comparison_matches.iter().any(|matched| {
                        matched["expectedKey"] == "bicycle-rented"
                            && matched["observedMessageId"] == persisted_event.event_id().as_str()
                    })
                    && comparison_matches.iter().any(|matched| {
                        matched["expectedKey"] == "rental-started"
                            && matched["observedMessageId"] == exact_integration_message_id
                    }),
                "comparison did not assign every expected key to its exact observed identity",
            )?;

            let mut mismatched_definition = canonical_definition.clone();
            mismatched_definition["id"] = json!("rent-available-bicycle-payload-mismatch");
            mismatched_definition["name"] = json!("Report a child payload mismatch");
            // Retain the canonical real-NATS budget: it includes resource reset,
            // fixture replay and transport retries, not just comparison. This
            // regression checks payload diagnostics rather than execution latency.
            mismatched_definition["expected"]["settleFor"] = json!("50ms");
            *mismatched_definition
                .pointer_mut("/expected/graphs/0/nodes/1/payload/bicycle_id")
                .ok_or_else(|| io::Error::other("canonical child payload is absent"))? =
                json!("bike-does-not-match");
            ensure(
                mismatched_definition["expected"]["graphs"][0]["nodes"][0]
                    == canonical_definition["expected"]["graphs"][0]["nodes"][0],
                "failure fixture changed the executable root command",
            )?;
            let response = app
                .clone()
                .oneshot(
                    authorize(Request::builder())
                        .method("POST")
                        .uri("/test-runs")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&mismatched_definition)?))?,
                )
                .await?;
            ensure(
                response.status() == StatusCode::OK,
                "behavioral mismatch was returned as an infrastructure error",
            )?;
            let failed_report = json_response(response).await?;
            ensure(
                failed_report["status"] == "failed"
                    && failed_report["comparison"]["status"] == "failed"
                    && failed_report["operation"]["status"] == "completed"
                    && failed_report["operation"]["result"]["decision"] == "accepted"
                    && failed_report["commandOutcome"]["outcome"]["status"] == "accepted",
                &format!("behavioral mismatch did not return a typed failed report for a valid root: {failed_report}"),
            )?;
            let payload_diagnostic = failed_report["comparison"]["diagnostics"]
                .as_array()
                .and_then(|diagnostics| {
                    diagnostics
                        .iter()
                        .find(|diagnostic| diagnostic["code"] == "payload-mismatch")
                })
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "payload-mismatch diagnostic is absent: {failed_report}"
                    ))
                })?;
            ensure(
                payload_diagnostic["path"] == "expected:bicycle-rented/payload"
                    && payload_diagnostic["expected"]
                        == json!({
                            "fleet_id": "city-fleet",
                            "bicycle_id": "bike-does-not-match"
                        })
                    && payload_diagnostic["observed"]
                        == json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
                "payload-mismatch diagnostic is not stable and structured",
            )?;

            for definition in definitions {
                let id = definition["id"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("behavioral test discovery omitted an id"))?;
                let revision = definition["revision"].as_str().ok_or_else(|| {
                    io::Error::other("behavioral test discovery omitted a revision")
                })?;
                let response = app
                    .clone()
                    .oneshot(
                        authorize(Request::builder())
                            .method("POST")
                            .uri(format!("/tests/{id}/runs"))
                            .body(Body::empty())?,
                    )
                    .await?;
                ensure(
                    response.status() == StatusCode::OK,
                    "behavioral test execution did not succeed",
                )?;
                let report = json_response(response).await?;
                if report["status"] != "passed" {
                    return Err(io::Error::other(format!(
                        "behavioral test `{id}` failed: {report}",
                    ))
                    .into());
                }
                ensure(
                    report["revision"] == revision,
                    "behavioral report used the wrong definition revision",
                )?;
                if id == "reject-duplicate-bicycle" {
                    assert_rejection_report(&report)?;
                }
                if id == "return-rented-bicycle" {
                    assert_fixture_replay_did_not_publish_integration_event(
                        &connection,
                        test_config,
                    )
                    .await?;
                }
            }
            Ok(())
        }
        .await;

        let stopped = host.shutdown().await;
        run_result?;
        stopped?;
        Ok(())
    }
    .await;
    let cleanup_result = cleanup_contexts(&connection, &test_configs).await;
    let drain_result = connection.drain().await;

    result?;
    cleanup_result?;
    drain_result?;
    Ok(())
}

#[tokio::test]
async fn integration_event_mapping_dispatches_a_command_through_nats() -> TestResult {
    let nats_url = required_nats_url()?;
    let scope = unique_scope()?;
    let application = format!("{scope}-reaction");
    let resource_limits = BikeRentalNatsResourceLimits::from_env()?;
    let config = BikeRentalNatsConfig::new_with_resource_limits(&application, resource_limits)?
        .with_fresh_completed_outcome_epoch("isolated-runtime-fixture")?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{scope}-reaction-integration"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let result: TestResult = async {
        let runtime = Arc::new(
            BikeRentalNatsRuntime::provision_config(connection.clone(), config.clone()).await?,
        );
        seed_demo(&runtime.store()).await?;

        let topology = runtime.config().messaging().topology().clone();
        let context = runtime.config().context();
        let route = runtime.config().integration_event_route();
        let reaction_config = ConsumerConfig::new(
            context.consumer_name("return-bicycle-after-rental", 1)?,
            context.durable_name("return-bicycle-after-rental", 1)?,
            route.address().clone(),
            Duration::from_secs(10),
            Duration::from_secs(5),
            1,
            5,
        )?;
        provision_durable_consumer(connection.jetstream(), &topology, &reaction_config).await?;
        let factory = connection.consumer_factory(topology.clone());
        factory.verify_consumer(&reaction_config).await?;
        let reaction_consumer =
            rostfrei::messaging::MessageConsumerFactory::create(&factory, reaction_config.clone())?;
        let messaging = Arc::new(connection.messaging_adapter(topology.clone()));
        let command_adapter: Arc<dyn CommandMessageAdapter> = messaging;
        let command_bus = CommandBus::new(context.clone(), command_adapter);
        let reaction_handler = Arc::new(
            IntegrationEventCommandHandler::<BicycleRentalStarted, _>::new(
                command_bus,
                reaction_config.durable_name().clone(),
                RetryDelay::new(Duration::from_millis(100))?,
                ReturnBicycleAfterRental,
            ),
        );
        let reaction_task =
            tokio::spawn(async move { reaction_consumer.run(reaction_handler).await });
        runtime.start_workers().await?;

        // Observe the normal application bus without submitting or registering
        // an operation through Tracer. The independent consumer must be ready
        // before the external command below can publish its first event.
        let feed = rostfrei::tracer::ObservationFeed::new(
            rostfrei::messaging::ApplicationName::new(&application)?,
            rostfrei::tracer::ObservationScope::Production,
        );
        let tracer = rostfrei::tracer::TracerBuilder::new(
            Arc::new(runtime.store()),
            rostfrei::DomainRegistry::new(),
        )
        .with_continuous_observation(Arc::clone(&feed))
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
        .build()?;
        let observation_task = runtime
            .start_correlation_observer(tracer.correlation_observer(OperationMode::Dispatch))
            .await?;
        ensure(
            feed.snapshot().status == rostfrei::tracer::ObservationStatus::Live,
            "observer was not ready before external publication",
        )?;
        let observation_app = http::router(
            tracer.clone(),
            HttpConfig::inspection_only("external-observation")?,
        );

        let receipt = runtime
            .transport()
            .invoke(
                invocation(
                    "rent-for-command-reaction",
                    "command-reaction-correlation",
                    RentBicycle::LOCAL_ID,
                    RentBicycle::SCHEMA_VERSION,
                    json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
                )?,
                Arc::new(RecordingObserver::default()),
            )
            .await?;
        ensure(
            matches!(receipt.outcome(), CommandOutcome::Accepted),
            "rental command was not accepted",
        )?;
        wait_for_history_len(&runtime, 4).await?;
        wait_for_consumer_acknowledgement(
            &connection,
            topology.integration_event_stream().as_str(),
            reaction_config.durable_name().as_str(),
        )
        .await?;
        wait_for_single_context_commands(&connection, &runtime).await?;

        let history = runtime.store().load(&demo_stream()).await?;
        ensure(
            history[2].event_type() == BicycleRented::LOCAL_ID,
            "integration source event was not BicycleRented",
        )?;
        ensure(
            history[3].event_type() == BicycleReturned::LOCAL_ID,
            "integration reaction did not produce BicycleReturned",
        )?;
        ensure(
            history[2].correlation_id() == history[3].correlation_id()
                && history[3]
                    .correlation_id()
                    .is_some_and(|id| id.as_str() == "command-reaction-correlation"),
            "integration command mapping did not preserve correlation",
        )?;
        let mut response_stream = connection
            .jetstream()
            .get_stream(topology.command_response_stream().as_str())
            .await?;
        let response_sequence = response_stream.info().await?.state.last_sequence;
        let response = response_stream.get_raw_message(response_sequence).await?;
        let generated_response: CommandResponse = serde_json::from_slice(&response.payload)?;
        ensure(
            matches!(
                generated_response.outcome(),
                CommandResponseOutcome::Accepted
            ) && history[3].correlation_id() == Some(generated_response.correlation_id()),
            "generated command response was not a correlated acceptance",
        )?;
        let reaction_causation = history[3]
            .causation_id()
            .ok_or_else(|| io::Error::other("reaction event omitted causation"))?;
        ensure(
            reaction_causation.as_str() == generated_response.command_message_id().as_str(),
            "reaction event did not use its generated command as direct causation",
        )?;
        let snapshot = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = feed.snapshot();
                if snapshot.items.iter().any(|flow| {
                    flow.correlation_id == "command-reaction-correlation" && flow.message_count >= 3
                }) {
                    return snapshot;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?;
        let flow = snapshot
            .items
            .iter()
            .find(|flow| flow.correlation_id == "command-reaction-correlation")
            .ok_or_else(|| io::Error::other("external flow was not discovered"))?;
        let response = observation_app
            .oneshot(
                Request::builder()
                    .uri(&flow.detail_href)
                    .header("authorization", "Bearer external-observation")
                    .body(Body::empty())?,
            )
            .await?;
        ensure(
            response.status() == StatusCode::OK,
            "external observation detail failed",
        )?;
        let detail = json_response(response).await?;
        let series: ObservedMessageSeries =
            serde_json::from_value(detail["messageSeries"].clone())?;
        ensure(
            series.messages().len() == 3,
            "expected both persisted events and the integration event",
        )?;
        ensure(
            series
                .messages()
                .iter()
                .all(|message| !message.is_command()),
            "passive capture fabricated a command",
        )?;
        ensure(
            series.command_outcomes().is_empty(),
            "passive capture fabricated a command outcome",
        )?;
        ensure(
            series.messages().iter().any(|message| {
                message.name() == "bicycle-rental-started"
                    && message.causation_id() == Some(history[2].event_id().as_str())
            }),
            "integration event lost its explicit domain-event parent",
        )?;
        ensure(
            tracer
                .correlation_mode("command-reaction-correlation")
                .is_err(),
            "external traffic unexpectedly registered a Tracer operation",
        )?;
        ensure(
            detail["partial"] == true && detail["fidelity"] == "grouped",
            "missing command evidence was described as complete",
        )?;
        observation_task.abort();
        let _ = observation_task.await;
        ensure(
            feed.snapshot().status == rostfrei::tracer::ObservationStatus::Unavailable,
            "stopped observer still appears live",
        )?;
        let mut quarantine_stream = connection
            .jetstream()
            .get_stream(topology.quarantine_stream().as_str())
            .await?;
        ensure(
            quarantine_stream.info().await?.state.messages == 0,
            "integration command reaction was quarantined",
        )?;

        runtime.stop_workers().await;
        reaction_task.abort();
        let _ = reaction_task.await;
        Ok(())
    }
    .await;
    let cleanup_result = cleanup(&connection, [&config]).await;
    let drain_result = connection.drain().await;

    result?;
    cleanup_result?;
    drain_result?;
    Ok(())
}

fn authorize(request: axum::http::request::Builder) -> axum::http::request::Builder {
    request.header("authorization", format!("Bearer {API_TOKEN}"))
}

async fn json_response(response: axum::response::Response) -> TestResult<Value> {
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(serde_json::from_slice(&bytes)?)
}

fn required_json_str<'a>(value: &'a Value, pointer: &str) -> TestResult<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("response omitted string `{pointer}`")).into())
}

fn observed_message(
    observed: &ObservedMessageSeries,
    kind: ExpectedMessageKind,
) -> TestResult<&rostfrei::tracer::ObservedMessageNode> {
    observed
        .messages()
        .iter()
        .find(|message| message.kind() == kind)
        .ok_or_else(|| io::Error::other("observed series omitted a required message kind").into())
}

fn assert_rejection_report(report: &Value) -> TestResult {
    let command_message_id = required_json_str(report, "/commandOutcome/commandMessageId")?;
    let response_message_id = required_json_str(report, "/commandOutcome/responseMessageId")?;
    let expected_response_message_id =
        command_response_message_id(&MessageId::new(command_message_id)?)?;
    ensure(
        response_message_id == expected_response_message_id.as_str()
            && report["operation"]["result"]["commandMessageId"] == command_message_id
            && report["operation"]["result"]["responseMessageId"] == response_message_id,
        "rejected command response identities are absent or not linked",
    )?;
    let expected_outcome = json!({
        "status": "rejected",
        "value": {
            "classification": "conflict",
            "code": "BICYCLE_ALREADY_IN_FLEET",
            "message": "The requested bicycle is already part of this fleet.",
            "details": {
                "bicycle_id": "bike-42",
                "code": "BICYCLE_ALREADY_IN_FLEET",
                "message": "The requested bicycle is already part of this fleet."
            }
        }
    });
    let expected_rejection = json!({
        "classification": "conflict",
        "code": "BICYCLE_ALREADY_IN_FLEET",
        "message": "The requested bicycle is already part of this fleet.",
        "details": {
            "bicycle_id": "bike-42",
            "code": "BICYCLE_ALREADY_IN_FLEET",
            "message": "The requested bicycle is already part of this fleet."
        }
    });
    if report["commandOutcome"]["outcome"] != expected_outcome
        || report["operation"]["result"]["rejection"] != expected_rejection
    {
        return Err(io::Error::other(format!(
            "rejected command outcome omitted classification, code, message, or details: commandOutcome={}, operationRejection={}",
            report["commandOutcome"]["outcome"], report["operation"]["result"]["rejection"]
        ))
        .into());
    }
    Ok(())
}

async fn run_isolation_test(
    connection: &NatsConnection,
    test_runtime: &BikeRentalNatsRuntime,
    production_runtime: &BikeRentalNatsRuntime,
) -> TestResult {
    let fixture = demo_fixture()?;
    ensure(
        test_runtime.config().application() == production_runtime.config().application(),
        "Test and Dispatch did not preserve one canonical application identity",
    )?;
    ensure(
        test_runtime
            .config()
            .context()
            .command_address(RentBicycle::LOCAL_ID)?
            .as_str()
            .contains(".test.command."),
        "Test command did not use the derived test subject scope",
    )?;
    ensure(
        !production_runtime
            .config()
            .context()
            .command_address(RentBicycle::LOCAL_ID)?
            .as_str()
            .contains(".test."),
        "Dispatch command unexpectedly used the test subject scope",
    )?;
    ensure(
        production_runtime.reset(&fixture).await.is_err(),
        "normal Dispatch resources accepted a destructive test reset",
    )?;
    let test_observer = Arc::new(RecordingObserver::default());
    let test_receipt = test_runtime
        .transport()
        .invoke(
            invocation(
                "test-rent",
                "test-rent-correlation",
                RentBicycle::LOCAL_ID,
                RentBicycle::SCHEMA_VERSION,
                json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
            )?,
            test_observer.clone(),
        )
        .await?;
    ensure(
        matches!(test_receipt.outcome(), CommandOutcome::Accepted),
        "Test command was not accepted",
    )?;
    ensure(
        test_observer.publications.lock().await.as_slice()
            == [CommandPublication::new(
                test_receipt.command_message_id(),
                test_receipt.duplicate(),
            )],
        "Test publication observation did not match its receipt",
    )?;
    wait_for_history_len(test_runtime, 3).await?;
    let test_history = test_runtime.store().load(&demo_stream()).await?;
    ensure(
        test_history[2]
            .correlation_id()
            .is_some_and(|correlation| correlation.as_str() == "test-rent-correlation"),
        "Test event did not preserve command correlation",
    )?;
    wait_for_integration_chain(
        connection,
        test_runtime.config(),
        &test_history[2],
        test_receipt.command_message_id(),
        1,
    )
    .await?;
    ensure(
        production_runtime.store().load(&demo_stream()).await?.len() == 2,
        "Test command changed production history",
    )?;

    let production_receipt = production_runtime
        .transport()
        .invoke(
            invocation(
                "production-add",
                "production-add-correlation",
                RegisterBicycle::LOCAL_ID,
                RegisterBicycle::SCHEMA_VERSION,
                json!({
                    "fleet_id": "city-fleet",
                    "bicycle_id": "production-bike",
                    "condition": "serviceable"
                }),
            )?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;
    ensure(
        matches!(production_receipt.outcome(), CommandOutcome::Accepted),
        "production command was not accepted",
    )?;
    wait_for_history_len(production_runtime, 3).await?;

    let test_durables_before_reset = durable_creations(connection, test_runtime).await?;
    let production_durables_before_reset =
        durable_creations(connection, production_runtime).await?;

    ensure(
        connection
            .delete_stream_if_exists(
                test_runtime
                    .config()
                    .messaging()
                    .topology()
                    .quarantine_stream()
                    .as_str(),
            )
            .await?,
        "pre-reset fixture stream was already absent",
    )?;
    test_runtime.reset(&demo_fixture()?).await?;
    ensure(
        test_runtime.store().load(&demo_stream()).await?.len() == 2,
        "Test reset did not restore the deterministic fixture history",
    )?;
    ensure(
        production_runtime.store().load(&demo_stream()).await?.len() == 3,
        "Test reset changed production history",
    )?;
    let test_durables_after_reset = durable_creations(connection, test_runtime).await?;
    let production_durables_after_reset = durable_creations(connection, production_runtime).await?;
    ensure(
        test_durables_before_reset.0 != test_durables_after_reset.0,
        "Test reset did not recreate the domain-event durable",
    )?;
    ensure(
        test_durables_before_reset.1 != test_durables_after_reset.1,
        "Test reset did not recreate the integration-event durable",
    )?;
    ensure(
        production_durables_before_reset == production_durables_after_reset,
        "Test reset recreated a production durable",
    )?;

    let reset_test_receipt = test_runtime
        .transport()
        .invoke(
            invocation(
                "test-rent-after-reset",
                "test-rent-after-reset-correlation",
                RentBicycle::LOCAL_ID,
                RentBicycle::SCHEMA_VERSION,
                json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
            )?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;
    ensure(
        matches!(reset_test_receipt.outcome(), CommandOutcome::Accepted),
        "Test command after reset was not accepted",
    )?;
    wait_for_history_len(test_runtime, 3).await?;
    let reset_test_history = test_runtime.store().load(&demo_stream()).await?;
    wait_for_integration_chain(
        connection,
        test_runtime.config(),
        &reset_test_history[2],
        reset_test_receipt.command_message_id(),
        1,
    )
    .await?;

    let production_rent_receipt = production_runtime
        .transport()
        .invoke(
            invocation(
                "production-rent-after-test-reset",
                "production-rent-after-test-reset-correlation",
                RentBicycle::LOCAL_ID,
                RentBicycle::SCHEMA_VERSION,
                json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
            )?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;
    ensure(
        matches!(production_rent_receipt.outcome(), CommandOutcome::Accepted),
        "production rental after Test reset was not accepted",
    )?;
    wait_for_history_len(production_runtime, 4).await?;
    let production_history = production_runtime.store().load(&demo_stream()).await?;
    wait_for_integration_chain(
        connection,
        production_runtime.config(),
        &production_history[3],
        production_rent_receipt.command_message_id(),
        1,
    )
    .await?;
    let reprovision = seed_demo(&production_runtime.store()).await?;
    ensure(
        reprovision.len() == 2
            && reprovision
                .iter()
                .all(|receipt| matches!(receipt, rostfrei::CommandReceipt::ExactReplay(_)))
            && production_runtime.store().load(&demo_stream()).await?.len() == 4,
        "production command seeding did not replay across extended business history",
    )?;
    wait_for_single_context_commands(connection, test_runtime).await?;
    wait_for_single_context_commands(connection, production_runtime).await
}

fn invocation(
    operation_id: &str,
    correlation_id: &str,
    command: &str,
    schema_version: u32,
    payload: Value,
) -> TestResult<CommandInvocation> {
    let context = match command {
        "collect-rental-payment" | "refund-eligible-rental-deposit" => FINANCE_CONTEXT,
        _ => RENTAL_CONTEXT,
    };
    let fingerprint = command_execution_fingerprint(context, command, schema_version, &payload);
    Ok(CommandInvocation::new(
        OperationId::new(operation_id)?,
        correlation_id,
        fingerprint,
        context,
        command,
        schema_version,
        payload,
    ))
}

async fn wait_for_history_len(runtime: &BikeRentalNatsRuntime, expected: usize) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if runtime.store().load(&demo_stream()).await?.len() == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for aggregate history").into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_consumer_acknowledgement(
    connection: &NatsConnection,
    stream: &str,
    durable: &str,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let info = consumer_info(connection, stream, durable).await?;
        if info.num_pending == 0 && info.num_ack_pending == 0 && info.ack_floor.stream_sequence > 0
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for consumer acknowledgement").into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_integration_chain(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
    source_event: &RecordedEvent,
    command_message_id: &str,
    expected_messages: u64,
) -> TestResult<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let route = config.integration_event_route();
    let expected_message_id = integration_message_id(
        route.address(),
        SchemaVersion::new(1)?,
        source_event.event_id(),
    )?;
    let expected_subject = IntegrationEventSubject::new(
        route.address().clone(),
        source_event
            .correlation_id()
            .ok_or_else(|| io::Error::other("source event has no correlation"))?
            .clone(),
        Some(CausationId::new(source_event.event_id().as_str())?),
    )?;
    loop {
        let mut integration_stream = connection
            .jetstream()
            .get_stream(
                config
                    .messaging()
                    .topology()
                    .integration_event_stream()
                    .as_str(),
            )
            .await?;
        if integration_stream.info().await?.state.messages == expected_messages {
            let raw = integration_stream
                .get_last_raw_message_by_subject(expected_subject.as_str())
                .await?;
            let integration_info = consumer_info(
                connection,
                config
                    .messaging()
                    .topology()
                    .integration_event_stream()
                    .as_str(),
                route.consumer().durable_name().as_str(),
            )
            .await?;
            let domain_info = consumer_info(
                connection,
                config.event_store().stream_name(),
                config.domain_event_consumer().durable_name().as_str(),
            )
            .await?;
            let envelope: IntegrationEventEnvelope<BicycleRentalStarted> =
                serde_json::from_slice(&raw.payload)?;
            let raw_message_id = raw.headers.get("Nats-Msg-Id").map(|value| value.as_str());
            let raw_correlation_id = raw
                .headers
                .get(CORRELATION_ID_HEADER)
                .map(|value| value.as_str());
            let integration_consumed = integration_info.num_pending == 0
                && integration_info.num_ack_pending == 0
                && integration_info.ack_floor.stream_sequence >= raw.sequence;
            let domain_consumed = domain_info.num_pending == 0
                && domain_info.num_ack_pending == 0
                && domain_info.ack_floor.stream_sequence >= source_event.stream_version().value();
            if integration_consumed && domain_consumed {
                ensure(
                    raw.subject.as_str() == expected_subject.as_str(),
                    "wrong integration subject",
                )?;
                ensure(
                    raw_message_id == Some(expected_message_id.as_str()),
                    "wrong integration transport message identity",
                )?;
                ensure(
                    raw_correlation_id
                        == source_event
                            .correlation_id()
                            .map(rostfrei::messaging::CorrelationId::as_str),
                    "wrong integration transport correlation identity",
                )?;
                ensure(
                    envelope.message_id() == &expected_message_id,
                    "wrong integration envelope message identity",
                )?;
                ensure(
                    Some(envelope.correlation_id()) == source_event.correlation_id(),
                    "integration envelope did not preserve correlation",
                )?;
                ensure(
                    source_event.causation_id().map(CausationId::as_str)
                        == Some(command_message_id),
                    "domain event did not preserve command causation",
                )?;
                ensure(
                    envelope.causation_id().map(CausationId::as_str)
                        == Some(source_event.event_id().as_str()),
                    "integration envelope did not use the source event as causation",
                )?;
                ensure(
                    envelope.payload().source_event_id() == source_event.event_id().as_str(),
                    "integration payload did not identify its committed event",
                )?;
                return Ok(expected_message_id.as_str().to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "timed out waiting for the correlated integration-event chain",
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn durable_creations(
    connection: &NatsConnection,
    runtime: &BikeRentalNatsRuntime,
) -> TestResult<(String, String)> {
    let domain = consumer_info(
        connection,
        runtime.config().event_store().stream_name(),
        runtime
            .config()
            .domain_event_consumer()
            .durable_name()
            .as_str(),
    )
    .await?;
    let integration = consumer_info(
        connection,
        runtime
            .config()
            .messaging()
            .topology()
            .integration_event_stream()
            .as_str(),
        runtime
            .config()
            .integration_event_route()
            .consumer()
            .durable_name()
            .as_str(),
    )
    .await?;
    Ok((domain.created, integration.created))
}

async fn consumer_info(
    connection: &NatsConnection,
    stream: &str,
    durable: &str,
) -> TestResult<ConsumerInfo> {
    let response = connection
        .client()
        .request(
            format!("$JS.API.CONSUMER.INFO.{stream}.{durable}"),
            "{}".into(),
        )
        .await?;
    let info: ConsumerInfo = serde_json::from_slice(&response.payload)?;
    ensure(
        info.name == durable,
        "consumer-info returned the wrong durable",
    )?;
    Ok(info)
}

async fn wait_for_command_stream_empty(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
    routes: Vec<&ConsumerConfig<CommandAddress>>,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stream_name = config.messaging().topology().command_stream().as_str();
    loop {
        let mut command_stream = connection.jetstream().get_stream(stream_name).await?;
        let stream_info = command_stream.info().await?;
        if stream_info.state.messages == 0 {
            ensure(
                config.messaging().commands().retention() == StreamRetention::WorkQueue,
                "command stream is not configured as a WorkQueue",
            )?;
            ensure(
                stream_info.state.consumer_count == routes.len(),
                "command WorkQueue has an unexpected passive consumer",
            )?;
            let mut worker_acknowledged = false;
            for route in &routes {
                let info =
                    consumer_info(connection, stream_name, route.durable_name().as_str()).await?;
                ensure(
                    info.num_pending == 0 && info.num_ack_pending == 0,
                    "command WorkQueue consumer is not idle after acknowledgement",
                )?;
                worker_acknowledged |= info.ack_floor.stream_sequence > 0;
            }
            ensure(
                worker_acknowledged,
                "no normal command worker acknowledged a WorkQueue command",
            )?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for command acknowledgement").into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_single_context_commands(
    connection: &NatsConnection,
    runtime: &BikeRentalNatsRuntime,
) -> TestResult {
    wait_for_command_stream_empty(
        connection,
        runtime.config(),
        runtime.runtime_config().commands.iter().collect(),
    )
    .await
}

async fn assert_fixture_replay_did_not_publish_integration_event(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    let topology = config.messaging().topology();
    loop {
        let domain_info = consumer_info(
            connection,
            config.event_store().stream_name(),
            config.domain_event_consumer().durable_name().as_str(),
        )
        .await?;
        let integration_info = consumer_info(
            connection,
            topology.integration_event_stream().as_str(),
            config
                .integration_event_route()
                .consumer()
                .durable_name()
                .as_str(),
        )
        .await?;
        if domain_info.num_pending == 0 && domain_info.num_ack_pending == 0 {
            let mut integration_stream = connection
                .jetstream()
                .get_stream(topology.integration_event_stream().as_str())
                .await?;
            ensure(
                integration_stream.info().await?.state.messages == 0
                    && integration_info.num_pending == 0
                    && integration_info.num_ack_pending == 0
                    && integration_info.ack_floor.stream_sequence == 0,
                "rented fixture replay published a new integration event",
            )?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "timed out waiting for rented-fixture domain events to be acknowledged",
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn cleanup<'a>(
    connection: &NatsConnection,
    configs: impl IntoIterator<Item = &'a BikeRentalNatsConfig>,
) -> TestResult {
    let mut first_error = None;
    for config in configs {
        let topology = config.messaging().topology();
        for stream in [
            topology.command_stream().as_str(),
            topology.command_response_stream().as_str(),
            topology.integration_event_stream().as_str(),
            topology.quarantine_stream().as_str(),
            config.event_store().stream_name(),
        ] {
            if let Err(error) = connection.jetstream().delete_stream(stream).await
                && first_error.is_none()
            {
                first_error = Some(error.to_string());
            }
        }
    }
    first_error.map_or_else(|| Ok(()), |error| Err(io::Error::other(error).into()))
}

async fn cleanup_contexts(
    connection: &NatsConnection,
    configs: &tracer::ContextConfigurations,
) -> TestResult {
    let mut result = cleanup(connection, [configs.rental()]).await;
    for (config, _) in configs
        .contexts()
        .filter(|(config, _)| config.context.name().as_str() != RENTAL_CONTEXT)
    {
        if let Err(error) = connection
            .jetstream()
            .delete_stream(config.event_store.stream_name())
            .await
            && result.is_ok()
        {
            result = Err(error.into());
        }
    }
    result
}

fn unique_scope() -> TestResult<String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(format!("brt-{:x}-{nanos:x}", process::id()))
}

fn ensure(condition: bool, message: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.to_owned()).into())
    }
}
