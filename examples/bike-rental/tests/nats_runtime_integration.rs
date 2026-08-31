#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::redundant_closure_for_method_calls,
    clippy::too_many_lines,
    reason = "integration assertions use bounded test data and retain end-to-end scenarios"
)]

use std::{
    env,
    error::Error,
    io,
    path::PathBuf,
    process,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bike_rental::{
    BicycleRentalStarted, BikeRentalCommand, BikeRentalNatsConfig, BikeRentalNatsResourceLimits,
    BikeRentalNatsRuntime,
    demo::demo_stream,
    rental_fleet::{AddBicycle, RentBicycle, RentalFleetAggregate, ReturnBicycle},
    tracer,
};
use http_body_util::BodyExt as _;
use rostfrei::{
    Aggregate, CommandDefinition, EventHistory, OperationId, RecordedEvent, StreamAggregateId,
    integration_message_id,
};
use rostfrei_messaging_core::{CausationId, IntegrationEventEnvelope, SchemaVersion};
use rostfrei_nats::{
    CORRELATION_ID_HEADER, NatsConnection, NatsConnectionConfig, ServerVersion, connect,
};
use rostfrei_tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandReceipt,
    CommandTransportObserver, ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository,
    OperationMode, TestRepository, TestScenarioReset, command_execution_fingerprint,
    http::{self, HttpConfig},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{sync::Mutex, time::Instant};
use tower::ServiceExt as _;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const API_TOKEN: &str = "behavioral-integration-capability";

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

#[tokio::test]
async fn command_workers_and_test_reset_are_subject_scope_isolated() -> TestResult {
    let Ok(nats_url) = env::var("ROSTFREI_NATS_URL") else {
        return Ok(());
    };
    let scope = unique_scope()?;
    let resource_limits = BikeRentalNatsResourceLimits::from_env()?;
    let test_config = BikeRentalNatsConfig::new_test_with_resource_limits(&scope, resource_limits)?;
    let production_config =
        BikeRentalNatsConfig::new_with_resource_limits(&scope, resource_limits)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{scope}-integration"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let result = async {
        let test_runtime = Arc::new(
            BikeRentalNatsRuntime::provision_test_with_resource_limits(
                connection.clone(),
                &scope,
                resource_limits,
            )
            .await?,
        );
        let production_runtime = Arc::new(
            BikeRentalNatsRuntime::provision_with_resource_limits(
                connection.clone(),
                &scope,
                resource_limits,
            )
            .await?,
        );
        test_runtime.seed_demo().await?;
        production_runtime.seed_demo().await?;
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
    let Ok(nats_url) = env::var("ROSTFREI_NATS_URL") else {
        return Ok(());
    };
    let scope = unique_scope()?;
    let resource_limits = BikeRentalNatsResourceLimits::from_env()?;
    let test_config = BikeRentalNatsConfig::new_test_with_resource_limits(&scope, resource_limits)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{scope}-behavioral-integration"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let result: TestResult = async {
        let test_runtime = Arc::new(
            BikeRentalNatsRuntime::provision_test_with_resource_limits(
                connection.clone(),
                &scope,
                resource_limits,
            )
            .await?,
        );
        let test_store = Arc::new(test_runtime.store().clone());
        let history: Arc<dyn EventHistory> = test_store.clone();
        let test_reset: Arc<dyn TestScenarioReset> = test_runtime.clone();
        let test_repository: Arc<dyn TestRepository> = Arc::new(FilesystemTestRepository::load(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tracer"),
        )?);
        let mut builder = tracer::builder(history)?
            .with_test_event_store(test_store)
            .with_test_transport(test_runtime.transport())
            .with_test_fixture("demo-fleet", test_reset)
            .with_test_repository(test_repository)
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
        builder.register_json::<RentBicycle>()?;
        builder.register_json::<ReturnBicycle>()?;
        builder.register_json::<AddBicycle>()?;
        let tracer = builder.build()?;
        let correlation_worker = test_runtime
            .start_correlation_observer(tracer.correlation_observer(OperationMode::Test))
            .await?;
        let app = http::router(tracer, HttpConfig::new(API_TOKEN)?);

        let run_result: TestResult = async {
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
                definitions.len() == 3,
                "expected exactly three bike-rental behavioral definitions",
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
            }
            Ok(())
        }
        .await;

        test_runtime.stop_workers().await;
        correlation_worker.abort();
        let _ = correlation_worker.await;
        run_result
    }
    .await;
    let cleanup_result = cleanup(&connection, [&test_config]).await;
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

async fn run_isolation_test(
    connection: &NatsConnection,
    test_runtime: &BikeRentalNatsRuntime,
    production_runtime: &BikeRentalNatsRuntime,
) -> TestResult {
    ensure(
        test_runtime.config().application() == production_runtime.config().application(),
        "Test and Dispatch did not preserve one canonical application identity",
    )?;
    ensure(
        test_runtime
            .config()
            .command_route(BikeRentalCommand::RentBicycle)
            .address()
            .as_str()
            .contains(".test.command."),
        "Test command did not use the derived test subject scope",
    )?;
    ensure(
        !production_runtime
            .config()
            .command_route(BikeRentalCommand::RentBicycle)
            .address()
            .as_str()
            .contains(".test."),
        "Dispatch command unexpectedly used the test subject scope",
    )?;
    ensure(
        production_runtime.reset().await.is_err(),
        "normal Dispatch resources accepted a destructive test reset",
    )?;
    let test_observer = Arc::new(RecordingObserver::default());
    let test_receipt = test_runtime
        .transport()
        .invoke(
            invocation(
                "test-rent",
                "test-rent-correlation",
                RentBicycle::COMMAND_NAME,
                RentBicycle::SCHEMA_VERSION,
                json!({"bicycle_id": "bike-42"}),
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
    wait_for_history_len(test_runtime, 2).await?;
    let test_history = test_runtime.store().load(&demo_stream()).await?;
    ensure(
        test_history[1]
            .correlation_id()
            .is_some_and(|correlation| correlation.as_str() == "test-rent-correlation"),
        "Test event did not preserve command correlation",
    )?;
    wait_for_integration_chain(connection, test_runtime, &test_history[1], &test_receipt, 1)
        .await?;
    ensure(
        production_runtime.store().load(&demo_stream()).await?.len() == 1,
        "Test command changed production history",
    )?;

    let production_receipt = production_runtime
        .transport()
        .invoke(
            invocation(
                "production-add",
                "production-add-correlation",
                AddBicycle::COMMAND_NAME,
                AddBicycle::SCHEMA_VERSION,
                json!({}),
            )?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;
    ensure(
        matches!(production_receipt.outcome(), CommandOutcome::Accepted),
        "production command was not accepted",
    )?;
    wait_for_history_len(production_runtime, 2).await?;

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
    test_runtime.reset().await?;
    ensure(
        test_runtime.store().load(&demo_stream()).await?.len() == 1,
        "Test reset did not restore deterministic seed history",
    )?;
    ensure(
        production_runtime.store().load(&demo_stream()).await?.len() == 2,
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
                RentBicycle::COMMAND_NAME,
                RentBicycle::SCHEMA_VERSION,
                json!({"bicycle_id": "bike-42"}),
            )?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;
    ensure(
        matches!(reset_test_receipt.outcome(), CommandOutcome::Accepted),
        "Test command after reset was not accepted",
    )?;
    wait_for_history_len(test_runtime, 2).await?;
    let reset_test_history = test_runtime.store().load(&demo_stream()).await?;
    wait_for_integration_chain(
        connection,
        test_runtime,
        &reset_test_history[1],
        &reset_test_receipt,
        1,
    )
    .await?;

    let production_rent_receipt = production_runtime
        .transport()
        .invoke(
            invocation(
                "production-rent-after-test-reset",
                "production-rent-after-test-reset-correlation",
                RentBicycle::COMMAND_NAME,
                RentBicycle::SCHEMA_VERSION,
                json!({"bicycle_id": "bike-42"}),
            )?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;
    ensure(
        matches!(production_rent_receipt.outcome(), CommandOutcome::Accepted),
        "production rental after Test reset was not accepted",
    )?;
    wait_for_history_len(production_runtime, 3).await?;
    let production_history = production_runtime.store().load(&demo_stream()).await?;
    wait_for_integration_chain(
        connection,
        production_runtime,
        &production_history[2],
        &production_rent_receipt,
        1,
    )
    .await?;
    wait_for_command_stream_empty(connection, test_runtime).await?;
    wait_for_command_stream_empty(connection, production_runtime).await
}

fn invocation(
    operation_id: &str,
    correlation_id: &str,
    command: &str,
    schema_version: u32,
    payload: Value,
) -> TestResult<CommandInvocation> {
    let aggregate_type = RentalFleetAggregate::aggregate_type().into_owned();
    let aggregate_id = StreamAggregateId::new("city-fleet")?;
    let fingerprint = command_execution_fingerprint(
        &aggregate_type,
        aggregate_id.as_str(),
        command,
        schema_version,
        &payload,
    );
    Ok(CommandInvocation::new(
        OperationId::new(operation_id)?,
        correlation_id,
        fingerprint,
        aggregate_type,
        aggregate_id,
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

async fn wait_for_integration_chain(
    connection: &NatsConnection,
    runtime: &BikeRentalNatsRuntime,
    source_event: &RecordedEvent,
    receipt: &CommandReceipt,
    expected_messages: u64,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    let route = runtime.config().integration_event_route();
    let expected_message_id = integration_message_id(
        route.address(),
        SchemaVersion::new(1)?,
        source_event.event_id(),
    )?;
    loop {
        let mut integration_stream = connection
            .jetstream()
            .get_stream(
                runtime
                    .config()
                    .messaging()
                    .topology()
                    .integration_event_stream()
                    .as_str(),
            )
            .await?;
        if integration_stream.info().await?.state.messages == expected_messages {
            let raw = integration_stream
                .get_last_raw_message_by_subject(route.address().as_str())
                .await?;
            let integration_info = consumer_info(
                connection,
                runtime
                    .config()
                    .messaging()
                    .topology()
                    .integration_event_stream()
                    .as_str(),
                route.consumer().durable_name().as_str(),
            )
            .await?;
            let domain_info = consumer_info(
                connection,
                runtime.config().event_store().stream_name(),
                runtime
                    .config()
                    .domain_event_consumer()
                    .durable_name()
                    .as_str(),
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
                    raw.subject.as_str() == route.address().as_str(),
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
                            .map(rostfrei_messaging_core::CorrelationId::as_str),
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
                        == Some(receipt.command_message_id()),
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
                return Ok(());
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
    runtime: &BikeRentalNatsRuntime,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stream_name = runtime
        .config()
        .messaging()
        .topology()
        .command_stream()
        .as_str();
    loop {
        let mut command_stream = connection.jetstream().get_stream(stream_name).await?;
        if command_stream.info().await?.state.messages == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for command acknowledgement").into());
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

fn unique_scope() -> TestResult<String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(format!("brt-{:x}-{nanos:x}", process::id()))
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
