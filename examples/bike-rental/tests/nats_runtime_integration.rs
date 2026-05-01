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
    fs, io,
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
    demo::{demo_fixture, demo_stream, rented_demo_fixture},
    rental_fleet::{AddBicycle, RentBicycle, RentalFleetAggregate, ReturnBicycle},
    tracer,
};
use http_body_util::BodyExt as _;
use rostfrei::{
    Aggregate, Command, EventHistory, OperationId, RecordedEvent, StreamAggregateId,
    command_message_id, command_response_message_id, integration_message_id,
};
use rostfrei_messaging_core::{
    CausationId, CorrelationId, IntegrationEventEnvelope, MessageId,
    OperationId as MessagingOperationId, SchemaVersion,
};
use rostfrei_nats::{
    CORRELATION_ID_HEADER, NatsConnection, NatsConnectionConfig, ServerVersion, StreamRetention,
    connect,
};
use rostfrei_tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandTransportObserver,
    ExpectedMessageKind, ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository,
    ObservedMessageSeries, OperationMode, TestRepository, TestScenarioReset,
    command_execution_fingerprint,
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
#[ignore = "requires NATS 2.12.1+"]
async fn command_workers_and_test_reset_are_subject_scope_isolated() -> TestResult {
    let nats_url = required_nats_url()?;
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
        let fixture = demo_fixture()?;
        test_runtime.apply_fixture(&fixture).await?;
        production_runtime.apply_fixture(&fixture).await?;
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
#[ignore = "requires NATS 2.12.1+"]
async fn behavioral_definitions_pass_through_http_and_the_isolated_nats_runtime() -> TestResult {
    let nats_url = required_nats_url()?;
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
            .with_test_scenario_reset(test_reset)
            .with_default_test_fixture(demo_fixture()?)
            .with_test_fixture(rented_demo_fixture()?)
            .with_test_repository(test_repository)
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
        builder.register_json::<RentalFleetAggregate, RentBicycle>()?;
        builder.register_json::<RentalFleetAggregate, ReturnBicycle>()?;
        builder.register_json::<RentalFleetAggregate, AddBicycle>()?;
        let tracer = builder.build()?;
        let correlation_worker = test_runtime
            .start_correlation_observer(tracer.correlation_observer(OperationMode::Test))
            .await?;
        let app = http::router(tracer, HttpConfig::new(API_TOKEN)?);

        let run_result: TestResult = async {
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
                definitions.len() == 3,
                "expected exactly three bike-rental behavioral definitions",
            )?;

            let canonical_bytes = fs::read(definition_root.join("rent-available-bicycle.json"))?;
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
            let aggregate_type = RentalFleetAggregate::aggregate_type().into_owned();
            let command_payload = json!({"bicycle_id": "bike-42"});
            let expected_command_message_id = command_message_id(
                test_runtime
                    .config()
                    .command_route(BikeRentalCommand::RentBicycle)
                    .address(),
                &MessagingOperationId::new(operation_id)?,
                command_execution_fingerprint(
                    &aggregate_type,
                    "city-fleet",
                    RentBicycle::COMMAND_NAME,
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
                    && report["operation"]["command"] == RentBicycle::COMMAND_NAME
                    && report["operation"]["schemaVersion"] == RentBicycle::SCHEMA_VERSION
                    && report["operation"]["aggregateType"] == aggregate_type
                    && report["operation"]["aggregateId"] == "city-fleet",
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
                .map(rostfrei_tracer::ObservedMessageNode::observation_order)
                .chain(
                    observed
                        .command_outcomes()
                        .iter()
                        .map(rostfrei_tracer::ObservedCommandOutcome::observation_order),
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
                    && command.name() == RentBicycle::COMMAND_NAME
                    && command.schema_version() == RentBicycle::SCHEMA_VERSION
                    && command.payload() == Some(&command_payload)
                    && command.aggregate().is_some_and(|aggregate| {
                        aggregate.aggregate_type == aggregate_type && aggregate.id == "city-fleet"
                    }),
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

            wait_for_history_len(&test_runtime, 2).await?;
            let history = test_runtime.store().load(&demo_stream()).await?;
            let fixture_event = history
                .first()
                .ok_or_else(|| io::Error::other("demo fixture domain event is absent"))?;
            ensure(
                fixture_event.event_type() == "rental-fleet-imported"
                    && fixture_event
                        .operation_id()
                        .as_str()
                        .starts_with("fixture:")
                    && fixture_event.stream_version().value() == 1
                    && fixture_event
                        .correlation_id()
                        .is_some_and(|id| id.as_str() == "fixture:demo-fleet:1")
                    && fixture_event.causation_id().is_none(),
                "inline run did not apply the deterministic demo MessageSeries fixture",
            )?;
            let persisted_event = history
                .get(1)
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
                &test_runtime,
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
            wait_for_command_stream_empty(&connection, &test_runtime).await?;

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
            mismatched_definition["expected"]["within"] = json!("3s");
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
                "behavioral mismatch did not return a typed failed report for a valid root",
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
                if id == "reject-unavailable-bicycle" {
                    assert_rejection_report(&report)?;
                }
                if id == "return-rented-bicycle" {
                    assert_fixture_replay_did_not_publish_integration_event(
                        &connection,
                        &test_runtime,
                    )
                    .await?;
                }
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

fn required_json_str<'a>(value: &'a Value, pointer: &str) -> TestResult<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("response omitted string `{pointer}`")).into())
}

fn observed_message(
    observed: &ObservedMessageSeries,
    kind: ExpectedMessageKind,
) -> TestResult<&rostfrei_tracer::ObservedMessageNode> {
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
            "code": "BICYCLE_UNAVAILABLE",
            "message": "The requested bicycle cannot currently be rented.",
            "details": {
                "bicycle_id": "bike-99",
                "code": "BICYCLE_UNAVAILABLE",
                "message": "The requested bicycle cannot currently be rented."
            }
        }
    });
    let expected_rejection = json!({
        "classification": "conflict",
        "code": "BICYCLE_UNAVAILABLE",
        "message": "The requested bicycle cannot currently be rented.",
        "details": {
            "bicycle_id": "bike-99",
            "code": "BICYCLE_UNAVAILABLE",
            "message": "The requested bicycle cannot currently be rented."
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
                RentBicycle::LOCAL_ID,
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
    wait_for_integration_chain(
        connection,
        test_runtime,
        &test_history[1],
        test_receipt.command_message_id(),
        1,
    )
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
                AddBicycle::LOCAL_ID,
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
    test_runtime.reset(&demo_fixture()?).await?;
    ensure(
        test_runtime.store().load(&demo_stream()).await?.len() == 1,
        "Test reset did not restore the deterministic fixture history",
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
                RentBicycle::LOCAL_ID,
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
        production_rent_receipt.command_message_id(),
        1,
    )
    .await?;
    let reprovision = production_runtime.apply_fixture(&demo_fixture()?).await?;
    ensure(
        reprovision.applied_domain_event_count() == 0
            && reprovision.reused_domain_event_count() == 1
            && production_runtime.store().load(&demo_stream()).await?.len() == 3,
        "production fixture provisioning did not tolerate extended business history",
    )?;
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
    command_message_id: &str,
    expected_messages: u64,
) -> TestResult<String> {
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
        let stream_info = command_stream.info().await?;
        if stream_info.state.messages == 0 {
            ensure(
                runtime.config().messaging().commands().retention() == StreamRetention::WorkQueue,
                "command stream is not configured as a WorkQueue",
            )?;
            ensure(
                stream_info.state.consumer_count == runtime.config().command_routes().len(),
                "command WorkQueue has an unexpected passive consumer",
            )?;
            let mut worker_acknowledged = false;
            for route in runtime.config().command_routes() {
                let info = consumer_info(
                    connection,
                    stream_name,
                    route.consumer().durable_name().as_str(),
                )
                .await?;
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

async fn assert_fixture_replay_did_not_publish_integration_event(
    connection: &NatsConnection,
    runtime: &BikeRentalNatsRuntime,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    let topology = runtime.config().messaging().topology();
    loop {
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
        let integration_info = consumer_info(
            connection,
            topology.integration_event_stream().as_str(),
            runtime
                .config()
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

fn unique_scope() -> TestResult<String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(format!("brt-{:x}-{nanos:x}", process::id()))
}

fn required_nats_url() -> TestResult<String> {
    match env::var("ROSTFREI_NATS_URL") {
        Ok(url) if !url.trim().is_empty() => Ok(url),
        Ok(_) => Err(io::Error::other("ROSTFREI_NATS_URL must not be empty").into()),
        Err(error) => Err(io::Error::other(format!(
            "ROSTFREI_NATS_URL is required for this ignored real-NATS test: {error}"
        ))
        .into()),
    }
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
