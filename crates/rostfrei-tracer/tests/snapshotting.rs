//! Real Tracer HTTP -> NATS worker -> snapshot/tail -> observed causal messages.
//! Run with `scripts/test_nats.py`, which owns and removes the disposable broker.
#![cfg(feature = "nats-host")]
#![allow(
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use async_trait::async_trait;
use http_body_util::BodyExt;
use reqwest::{Client, Method, Url};
use rostfrei::{
    Aggregate, AggregateCodecs, ApplicationName, CommandDecision, CommandExecution,
    CommandExecutor, CommandHandler, CommandHandlingResult, CommittedDomainEvent, EventHistory,
    IntegrationEvent, IntegrationEventMapper, RebuildDiagnostics, SnapshotPolicy, SnapshotStatus,
    SnapshotStore, StreamAggregateId, StreamAggregateType, StreamId, StreamVersion,
};
use rostfrei_nats::{
    NatsConnectionConfig, NatsReadModelConfig, NatsSnapshotStore, connect, provision_read_model,
};
use rostfrei_tracer::{
    ApplicationBindings, ExposeTracePayloadsForLocalDevelopment, Fixture, NatsApplication,
    ObservationFeed, ObservationScope, TracerConfig, http::HttpConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    error::Error,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

rostfrei::install_macro_support!();
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const TOKEN: &str = "owned-snapshot-regression";

#[derive(rostfrei::BoundedContext)]
#[domain(id = "snapshot-counter", label = "Snapshot counter")]
struct CounterContext;

#[derive(rostfrei::Command)]
#[domain(context = CounterContext, id = "increment", label = "Increment")]
struct Increment {
    id: String,
    amount: u64,
}

#[derive(Serialize, Deserialize, rostfrei::DomainEvent)]
#[domain(id = "incremented", label = "Incremented")]
struct Incremented {
    amount: u64,
    total: u64,
}

#[derive(rostfrei::AggregateEvents)]
enum CounterEvents {
    Incremented(Incremented),
}

struct Counter;
impl Aggregate for Counter {
    type State = u64;
    type Event = CounterEvents;
    const BOUNDED_CONTEXT: &'static str = "snapshot-counter";
    const AGGREGATE_TYPE: &'static str = "snapshot-counter/counter";
    fn initial(_: &StreamId) -> u64 {
        0
    }
    fn apply(state: &mut u64, event: &CounterEvents) {
        let CounterEvents::Incremented(event) = event;
        // A wrong cached prefix changes the result even when the tail is present.
        *state = state.saturating_add(event.amount);
    }
}

#[derive(rostfrei::DomainError)]
#[domain(
    id = "refused",
    label = "Refused",
    code = "REFUSED",
    message = "Refused"
)]
struct Refused;

struct IncrementHandler;
#[async_trait]
impl CommandHandler<Increment> for IncrementHandler {
    type Rejection = Refused;
    async fn handle(
        &self,
        command: &Increment,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Refused> {
        let mut counter = execution.load::<Counter>(&command.id).await?;
        let total = counter.state().saturating_add(command.amount);
        counter.raise(CounterEvents::Incremented(Incremented {
            amount: command.amount,
            total,
        }));
        Ok(CommandDecision::Accepted)
    }
}

#[derive(Serialize, Deserialize)]
struct TotalChanged {
    total: u64,
}
impl IntegrationEvent for TotalChanged {
    const EVENT_NAME: &'static str = "total-changed";
    const SCHEMA_VERSION: u32 = 1;
}
struct PublishTotal;
impl IntegrationEventMapper<Incremented> for PublishTotal {
    type Output = TotalChanged;
    fn map(&self, event: &CommittedDomainEvent<'_, Incremented>) -> TotalChanged {
        TotalChanged {
            total: event.event().total,
        }
    }
}

fn bindings(codecs: Arc<AggregateCodecs>) -> TestResult<Arc<ApplicationBindings>> {
    let mut bindings = ApplicationBindings::new().with_event_codecs(codecs);
    bindings.register_aggregate::<Counter>()?;
    bindings.register_command::<Increment, _>(
        IncrementHandler,
        rostfrei::JsonDomainRejectionMapper::new(
            rostfrei::CommandRejectionClassification::Conflict,
        ),
    )?;
    bindings.register_integration_event::<Counter, Incremented, _>(PublishTotal);
    Ok(Arc::new(bindings))
}

fn fixture(id: &str, amount: u64) -> TestResult<Fixture> {
    Ok(serde_json::from_value(json!({
        "schemaVersion": 1, "id": id, "revision": "1",
        "messages": (1_u64..=6).map(|version| json!({
            "kind": "domain-event", "messageId": format!("{id}-{version}"),
            "correlationId": "fixture-history", "name": "incremented", "schemaVersion": 1,
            "aggregate": {"type": "snapshot-counter/counter", "id": "one"},
            "streamVersion": version,
            "payload": {"amount": amount, "total": amount.saturating_mul(version)}
        })).collect::<Vec<_>>()
    }))?)
}

fn definition() -> Value {
    json!({
        "schemaVersion": 2, "id": "snapshot-counter-increment", "name": "Increment with snapshot configuration",
        "setup": {"fixture": "six-units"},
        "expected": {"within": "10s", "settleFor": "100ms", "graphs": [{"nodes": [
            {"kind": "command", "key": "increment", "context": "snapshot-counter",
             "name": "increment", "schemaVersion": 1, "payload": {"id": "one", "amount": 5}, "outcome": "accepted"},
            {"kind": "domain-event", "key": "incremented", "parentKey": "increment",
             "name": "incremented", "schemaVersion": 1, "payload": {"amount": 5, "total": 11}},
            {"kind": "integration-event", "key": "published", "parentKey": "incremented",
             "name": "total-changed", "schemaVersion": 1, "payload": {"total": 11}}
        ]}]}
    })
}

struct Api {
    client: Client,
    origin: Url,
}
impl Api {
    fn url(&self, href: &str) -> TestResult<Url> {
        assert!(href.starts_with('/') && !href.starts_with("//"));
        assert!(!href.contains(['\\', '#', '{', '}']) && !href.chars().any(char::is_control));
        let url = self.origin.join(href)?;
        assert_eq!(url.origin(), self.origin.origin());
        Ok(url)
    }

    async fn json(&self, method: Method, href: &str, body: Option<&Value>) -> TestResult<Value> {
        let mut request = self
            .client
            .request(method, self.url(href)?)
            .bearer_auth(TOKEN);
        if let Some(body) = body {
            request = request.json(body);
        }
        Ok(request.send().await?.error_for_status()?.json().await?)
    }

    async fn command(&self, link: &str, id: &str, total: u64) -> TestResult<Value> {
        let response = self
            .client
            .post(self.url(link)?)
            .bearer_auth(TOKEN)
            .header("Idempotency-Key", id)
            .json(&command_request())
            .send()
            .await?
            .error_for_status()?;
        let location = response
            .headers()
            .get("location")
            .ok_or("operation location")?
            .to_str()?
            .to_owned();
        let operation: Value = response.json().await?;
        let capture = self
            .json(
                Method::GET,
                &format!(
                    "{}?within=10s&settleFor=100ms",
                    href(&operation, "messageSeriesHref")?
                ),
                None,
            )
            .await?;
        assert_eq!(capture["capture"]["settled"], true, "{capture:#}");
        assert_eq!(capture["capture"]["fidelity"], "exact", "{capture:#}");
        let messages = capture["messageSeries"]["messages"]
            .as_array()
            .ok_or("messages")?;
        assert_eq!(
            messages.len(),
            3,
            "only the command and its new domain/integration events"
        );
        let command = messages
            .iter()
            .find(|m| m["kind"] == "command")
            .ok_or("observed command")?;
        let domain = messages
            .iter()
            .find(|m| m["kind"] == "domain-event")
            .ok_or("observed domain event")?;
        let integration = messages
            .iter()
            .find(|m| m["kind"] == "integration-event")
            .ok_or("observed integration event")?;
        assert_eq!(domain["causationId"], command["messageId"]);
        assert_eq!(integration["causationId"], domain["messageId"]);
        assert_eq!(domain["payload"]["total"], total);
        assert_eq!(integration["payload"]["total"], total);
        let completed = self.json(Method::GET, &location, None).await?;
        assert_eq!(completed["status"], "completed", "{completed:#}");
        assert_eq!(completed["result"]["decision"], "accepted");
        eprintln!(
            "real Test: total={total}, settled exact command -> incremented -> total-changed"
        );
        Ok(completed)
    }
}

fn href<'a>(document: &'a Value, field: &str) -> TestResult<&'a str> {
    document[field]
        .as_str()
        .ok_or_else(|| format!("missing {field}").into())
}
fn command_request() -> Value {
    json!({"schemaVersion": 1, "payload": {"id": "one", "amount": 5}})
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "snapshot fixture and replay acceptance across isolated runtimes"
)]
async fn snapshot_fixtures_and_inspection_preserve_full_replay() -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "snapshot-fixtures",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new(format!(
        "snapshot-fixtures-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ))?;
    let context = application.test_bounded_context("snapshot-counter")?;
    let config = NatsReadModelConfig::new(&context, "snapshots")?;
    provision_read_model(connection.jetstream(), &config).await?;
    let reports = Arc::new(RebuildDiagnostics::new(64));
    let snapshot_store =
        Arc::new(NatsSnapshotStore::connect(connection.jetstream().clone(), config).await?);
    let mut codecs = AggregateCodecs::default()
        .with_snapshot_store(snapshot_store.clone())
        .with_rebuild_diagnostics(reports.clone());
    codecs.register_json_snapshot::<Counter>("counter-v1", 1, SnapshotPolicy::Manual)?;
    let codecs = Arc::new(codecs);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let api = Api {
        origin: Url::parse(&format!("http://{}", listener.local_addr()?))?,
        client: Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(40))
            .build()?,
    };
    let host = NatsApplication::builder(context, connection.clone(), bindings(codecs.clone())?)?
        .with_fresh_completed_outcome_epoch("fixture-snapshots")?
        .with_default_test_fixture(fixture("six-units", 1)?)
        .with_tracer(Some(
            TracerConfig::new(
                listener,
                HttpConfig::new(TOKEN)?.with_inspection_token("inspect")?,
            )
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment)),
        ))
        .provision_and_start()
        .await?;
    let catalog = api.json(Method::GET, "/catalog", None).await?;
    let suite = api
        .json(
            Method::POST,
            href(&catalog["behavioralTest"], "runHref")?,
            Some(&definition()),
        )
        .await?;
    assert_eq!(suite["status"], "passed");
    let stream = StreamId::new(
        StreamAggregateType::new(Counter::AGGREGATE_TYPE)?,
        StreamAggregateId::new("one")?,
    );
    let executor = CommandExecutor::with_codecs(host.runtime().store(), codecs.clone());
    executor
        .create_snapshot::<Counter>(&stream, StreamVersion::new(4))
        .await?;
    let capability = &catalog["historical"][0];
    assert!(
        capability["aggregateHref"].is_string(),
        "Historical must advertise snapshot inspection and state reconstruction"
    );
    let query = "aggregateType=snapshot-counter%2Fcounter&aggregateId=one&version=6";
    let aggregate_href = format!("{}?{query}", href(capability, "aggregateHref")?);
    let originals = host.runtime().store().load(&stream).await?;
    let assisted = api
        .json(
            Method::GET,
            &format!("{aggregate_href}&useSnapshots=true"),
            None,
        )
        .await?;
    assert_eq!(assisted["state"], 6);
    assert_eq!(assisted["version"], "6");
    assert_eq!(assisted["snapshotVersion"], "4");
    assert_eq!(assisted["snapshotStatus"], "hit");
    assert_eq!(assisted["eventsApplied"], 2);
    assert_eq!(assisted["snapshots"][0]["streamVersion"], "4");
    assert_eq!(assisted["snapshots"][0]["state"], 4);
    let full = api
        .json(
            Method::GET,
            &format!("{aggregate_href}&useSnapshots=false"),
            None,
        )
        .await?;
    assert_eq!(full["state"], assisted["state"]);
    assert_eq!(full["snapshotVersion"], "0");
    assert_eq!(full["snapshotStatus"], "disabled");
    assert_eq!(full["eventsApplied"], 6);
    assert_eq!(
        full["snapshots"], assisted["snapshots"],
        "turning replay acceleration off must not hide stored snapshots"
    );
    let exported = api
        .json(
            Method::GET,
            &format!(
                "{}?{query}&snapshotVersion=4&fixtureId=snapshot-six&generation={}",
                href(capability, "snapshotFixtureHref")?,
                assisted["generation"].as_str().unwrap()
            ),
            None,
        )
        .await?;
    assert_eq!(
        exported["messages"].as_array().unwrap().len(),
        6,
        "snapshot fixtures keep their complete backing history"
    );
    assert_eq!(exported["snapshots"][0]["streamVersion"], "4");
    assert!(exported["snapshots"][0]["state"].is_object());
    assert_eq!(
        host.runtime().store().load(&stream).await?,
        originals,
        "all inspection and exports are read-only"
    );
    let wrong_scope = api
        .client
        .get(api.url(&aggregate_href)?)
        .bearer_auth("inspect")
        .send()
        .await?;
    assert_eq!(wrong_scope.status(), reqwest::StatusCode::FORBIDDEN);
    inspect_invalid_snapshots(host.runtime(), snapshot_store.as_ref(), &stream).await?;
    fixture_boundaries(
        &connection,
        &application,
        host.runtime(),
        codecs.clone(),
        &exported,
    )
    .await?;

    // The exported state must remain valid after a fresh fixture identity/authority.
    let imported: Fixture = serde_json::from_value(exported.clone())?;
    let mut cached = serde_json::to_value(fixture("cached-six", 1)?)?;
    cached["snapshots"] =
        json!([{"aggregate":{"type":Counter::AGGREGATE_TYPE,"id":"one"},"streamVersion":"6"}]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let copy_api = Api {
        origin: Url::parse(&format!("http://{}", listener.local_addr()?))?,
        client: api.client.clone(),
    };
    let copy_application = ApplicationName::new(format!("{}-copy", application.as_str()))?;
    let copied = NatsApplication::builder(
        copy_application.test_bounded_context("snapshot-counter")?,
        connection.clone(),
        bindings(codecs.clone())?,
    )?
    .with_fresh_completed_outcome_epoch("fixture-copy")?
    .with_default_test_fixture(imported)
    .with_test_fixture(serde_json::from_value(cached)?)
    .with_tracer(Some(
        TracerConfig::new(listener, HttpConfig::new(TOKEN)?)
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment)),
    ))
    .provision_and_start()
    .await?;
    let copy_catalog = copy_api.json(Method::GET, "/catalog", None).await?;
    let mut behavior = definition();
    behavior["setup"]["fixture"] = json!("snapshot-six");
    let outcome = copy_api
        .json(
            Method::POST,
            href(&copy_catalog["behavioralTest"], "runHref")?,
            Some(&behavior),
        )
        .await?;
    assert_eq!(outcome["status"], "passed", "{outcome:#}");
    assert_rebuild(&reports, SnapshotStatus::Hit, 4, 6, 2);
    let second = copy_api
        .json(
            Method::POST,
            href(&copy_catalog["behavioralTest"], "runHref")?,
            Some(&behavior),
        )
        .await?;
    assert_eq!(second["status"], "passed", "{second:#}");
    assert_rebuild(&reports, SnapshotStatus::Hit, 4, 6, 2);
    behavior["setup"]["fixture"] = json!("cached-six");
    let outcome = copy_api
        .json(
            Method::POST,
            href(&copy_catalog["behavioralTest"], "runHref")?,
            Some(&behavior),
        )
        .await?;
    assert_eq!(outcome["status"], "passed", "{outcome:#}");
    assert_rebuild(&reports, SnapshotStatus::Hit, 6, 6, 0);
    assert_eq!(copied.runtime().store().load(&stream).await?.len(), 7);
    let reset = href(&catalog["testScenario"], "fixtureResetHrefTemplate")?
        .replace("{fixtureId}", "six-units");
    assert_eq!(
        api.client
            .post(api.url(&reset)?)
            .bearer_auth(TOKEN)
            .send()
            .await?
            .status(),
        reqwest::StatusCode::NO_CONTENT
    );
    for link in [
        format!(
            "{aggregate_href}&generation={}",
            assisted["generation"].as_str().unwrap()
        ),
        format!(
            "{}?{query}&snapshotVersion=4&fixtureId=stale&generation={}",
            href(capability, "snapshotFixtureHref")?,
            assisted["generation"].as_str().unwrap()
        ),
    ] {
        assert_eq!(
            api.client
                .get(api.url(&link)?)
                .bearer_auth(TOKEN)
                .send()
                .await?
                .status(),
            reqwest::StatusCode::GONE
        );
    }
    let fresh = api.json(Method::GET, &aggregate_href, None).await?;
    assert_eq!(fresh["snapshots"], json!([]));
    assert_eq!(fresh["state"], 6);
    eprintln!(
        "Snapshot fixtures: imported state + retained events, cached event fixtures, repeated resets and full/snapshot replay equivalence passed"
    );
    copied.shutdown().await?;
    host.shutdown().await?;
    connection.drain().await?;
    Ok(())
}

async fn router_get(app: &axum::Router, href: &str, token: &str) -> TestResult<(u16, Value)> {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(href)
                .header("Authorization", format!("Bearer {token}"))
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status = response.status().as_u16();
    Ok((
        status,
        serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?,
    ))
}

async fn inspect_invalid_snapshots(
    runtime: &rostfrei_tracer::NatsRuntime,
    snapshots: &NatsSnapshotStore,
    stream: &StreamId,
) -> TestResult {
    let store = runtime.store();
    let key = rostfrei::SnapshotKey::new(
        store.history_authority().ok_or("authority")?,
        stream,
        "counter-v1",
        1,
    );
    for (version, revision, schema, data) in [
        (3, "counter-v1", 1, b"{}".to_vec()),
        (2, "obsolete", 2, b"2".to_vec()),
    ] {
        let checkpoint = store
            .load_replay(stream, None, Some(StreamVersion::new(version)))
            .await?
            .checkpoint
            .ok_or("checkpoint")?;
        snapshots
            .save(
                &key,
                &rostfrei::StoredSnapshot::new(revision.into(), schema, checkpoint, data)?,
            )
            .await?;
    }
    let app = rostfrei_tracer::http::router(
        runtime
            .tracer_builder()
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
            .build()?,
        HttpConfig::new(TOKEN)?,
    );
    let (_, page) = router_get(&app, "/historical/test/aggregate?aggregateType=snapshot-counter%2Fcounter&aggregateId=one&version=3", TOKEN).await?;
    assert_eq!(
        page["state"], 3,
        "malformed typed snapshot must fall back to original events"
    );
    assert_eq!(page["snapshotStatus"], "invalid-state");
    for snapshot in page["snapshots"]
        .as_array()
        .ok_or("snapshots")?
        .iter()
        .filter(|s| s["streamVersion"] != "4")
    {
        assert_eq!(
            snapshot["valid"], false,
            "incompatible or undecodable snapshot must not be presented as valid"
        );
        assert!(snapshot.get("state").is_none());
    }
    Ok(())
}

async fn fixture_boundaries(
    connection: &rostfrei_nats::NatsConnection,
    application: &ApplicationName,
    test: &rostfrei_tracer::NatsRuntime,
    codecs: Arc<AggregateCodecs>,
    exported: &Value,
) -> TestResult {
    let bindings = bindings(codecs.clone())?;
    let config = rostfrei_tracer::NatsRuntimeConfig::new(
        application.bounded_context("snapshot-counter")?,
        &bindings,
        1,
    )?
    .with_fresh_completed_outcome_epoch("snapshot-production")?;
    let normal =
        rostfrei_tracer::NatsRuntime::provision(connection.clone(), config, bindings).await?;
    let stream = StreamId::new(
        StreamAggregateType::new(Counter::AGGREGATE_TYPE)?,
        StreamAggregateId::new("one")?,
    );
    for (field, value) in [
        ("data", json!("OTk5")),
        ("data", json!("e30=")),
        ("replayRevision", json!("counter-obsolete")),
        ("schemaVersion", json!(2)),
    ] {
        let mut invalid = exported.clone();
        invalid["snapshots"][0]["state"][field] = value;
        assert!(
            normal
                .apply_fixture(&serde_json::from_value(invalid)?)
                .await
                .is_err()
        );
        assert!(
            normal.store().load(&stream).await?.is_empty(),
            "invalid imported state must fail before any event writes"
        );
    }
    normal.apply_fixture(&fixture("normal-tens", 10)?).await?;
    CommandExecutor::with_codecs(normal.store(), codecs)
        .create_snapshot::<Counter>(&stream, StreamVersion::new(4))
        .await?;
    let before = normal.store().load(&stream).await?;
    let app = rostfrei_tracer::http::router(
        test.tracer_builder()
            .with_historical_reader(normal.historical_reader())
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
            .build()?,
        HttpConfig::new(TOKEN)?
            .with_inspection_token("inspect")?
            .with_dispatch_token("dispatch")?,
    );
    let (_, catalog) = router_get(&app, "/catalog", "inspect").await?;
    let query = "aggregateType=snapshot-counter%2Fcounter&aggregateId=one&version=6";
    let link = format!(
        "{}?{query}",
        href(&catalog["historical"][0], "aggregateHref")?
    );
    let (status, page) = router_get(&app, &link, "inspect").await?;
    assert_eq!(status, 200);
    assert_eq!(page["state"], 60);
    assert_eq!(page["snapshots"][0]["state"], 40);
    assert_eq!(page["snapshots"].as_array().ok_or("snapshots")?.len(), 1);
    assert_eq!(page["eventsApplied"], 2);
    for token in [TOKEN, "dispatch"] {
        assert_eq!(router_get(&app, &link, token).await?.0, 403);
    }
    let redacted = rostfrei_tracer::http::router(
        test.tracer_builder()
            .with_historical_reader(normal.historical_reader())
            .build()?,
        HttpConfig::new(TOKEN)?.with_inspection_token("inspect")?,
    );
    let (_, page) = router_get(&redacted, &link, "inspect").await?;
    assert!(page.get("state").is_none());
    assert!(page["snapshots"][0].get("state").is_none());
    assert_eq!(page["snapshots"][0]["valid"], true);
    let export = format!(
        "{}?{query}&snapshotVersion=4&fixtureId=normal-copy&generation={}",
        href(&catalog["historical"][0], "snapshotFixtureHref")?,
        page["generation"].as_str().ok_or("generation")?
    );
    assert_eq!(router_get(&redacted, &export, "inspect").await?.0, 403);
    assert_eq!(router_get(&app, &export, "inspect").await?.0, 200);
    for suffix in ["&version=01", "&snapshotVersion=4", "&generation=stale"] {
        let link = format!(
            "{}?aggregateType=snapshot-counter%2Fcounter&aggregateId=one{suffix}",
            href(&catalog["historical"][0], "aggregateHref")?
        );
        assert_ne!(router_get(&app, &link, "inspect").await?.0, 200);
    }
    assert_eq!(
        normal.store().load(&stream).await?,
        before,
        "inspection/export preserve Production history"
    );
    Ok(())
}

fn assert_rebuild(
    reports: &RebuildDiagnostics,
    status: SnapshotStatus,
    snapshot: u64,
    head: u64,
    events: usize,
) {
    let report = reports.recent().pop().unwrap();
    eprintln!(
        "rebuild: status={:?}, snapshot={}, head={}, read={}, applied={}",
        report.snapshot_status,
        report.snapshot_version,
        report.head_version,
        report.events_read,
        report.events_applied
    );
    assert!(report.error.is_none(), "{report:?}");
    assert_eq!(report.snapshot_status, status, "{report:?}");
    assert!(
        report.authority.is_some(),
        "snapshot selection needs the current history authority"
    );
    assert_eq!(
        (report.snapshot_version, report.head_version),
        (snapshot, head)
    );
    assert_eq!(
        (report.events_read, report.events_applied),
        (events, events)
    );
}

async fn simulate(api: &Api, link: &str, total: u64, head: u64) -> TestResult {
    let result = api
        .json(Method::POST, link, Some(&command_request()))
        .await?;
    assert_eq!(result["kind"], "handlerSimulation");
    assert_eq!(result["committed"], false);
    assert_eq!(result["outcome"]["decision"], "accepted");
    assert_eq!(result["streams"][0]["baseStreamVersion"], head);
    assert_eq!(
        result["proposedEvents"][0]["proposedStreamVersion"],
        head.saturating_add(1)
    );
    assert_eq!(result["proposedEvents"][0]["payload"]["total"], total);
    assert!(result.get("operationId").is_none());
    assert!(result.get("messageSeries").is_none());
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one HTTP/NATS journey across a Test authority reset"
)]
async fn tracer_snapshot_hits_preserve_commands_observation_and_reset_isolation() -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "tracer-snapshots",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new(format!(
        "tracer-snapshots-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ))?;
    let context = application.test_bounded_context("snapshot-counter")?;
    let snapshot_config = NatsReadModelConfig::new(&context, "snapshots")?;
    provision_read_model(connection.jetstream(), &snapshot_config).await?;
    let snapshots = Arc::new(
        NatsSnapshotStore::connect(connection.jetstream().clone(), snapshot_config).await?,
    );
    let reports = Arc::new(RebuildDiagnostics::new(64));
    let mut codecs = AggregateCodecs::default()
        .with_snapshot_store(snapshots)
        .with_rebuild_diagnostics(reports.clone());
    codecs.register_json_snapshot::<Counter>("counter-v1", 1, SnapshotPolicy::Manual)?;
    let codecs = Arc::new(codecs);
    let feed = ObservationFeed::new(application, ObservationScope::Test);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let api = Api {
        origin: Url::parse(&format!("http://{}", listener.local_addr()?))?,
        client: Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(40))
            .build()?,
    };
    let host = NatsApplication::builder(context, connection.clone(), bindings(codecs.clone())?)?
        .with_fresh_completed_outcome_epoch("tracer-snapshot-regression")?
        .with_default_test_fixture(fixture("six-units", 1)?)
        .with_test_fixture(fixture("six-tens", 10)?)
        .with_tracer(Some(
            TracerConfig::new(listener, HttpConfig::new(TOKEN)?)
                .with_continuous_observation(feed.clone())
                .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment)),
        ))
        .provision_and_start()
        .await?;

    let catalog = api.json(Method::GET, "/catalog", None).await?;
    assert_eq!(catalog["catalogVersion"], 1);
    let command = &catalog["contexts"][0]["commands"][0]["versions"][0];
    assert_eq!(command["schemaVersion"], 1);
    let test = href(command, "testHrefTemplate")?;
    let simulation = href(command, "simulateHandlerHrefTemplate")?;
    let behavior = &catalog["behavioralTest"];
    let schema = api
        .json(Method::GET, href(behavior, "schemaHref")?, None)
        .await?;
    assert!(schema.is_object());
    api.json(
        Method::POST,
        href(behavior, "validateHref")?,
        Some(&definition()),
    )
    .await?;
    let suite = api
        .json(
            Method::POST,
            href(behavior, "runHref")?,
            Some(&definition()),
        )
        .await?;
    assert_eq!(suite["status"], "passed", "{suite:#}");
    assert_eq!(suite["comparison"]["status"], "passed", "{suite:#}");
    eprintln!("JSON v2 behavior suite: passed with snapshots configured");
    assert_rebuild(&reports, SnapshotStatus::Miss, 0, 6, 6);

    let stream = StreamId::new(
        StreamAggregateType::new(Counter::AGGREGATE_TYPE)?,
        StreamAggregateId::new("one")?,
    );
    let old_store = host.runtime().store();
    let old_authority = old_store.history_authority().ok_or("authority")?.to_owned();
    let executor = CommandExecutor::with_codecs(old_store.clone(), codecs.clone());
    let original = old_store.load(&stream).await?;
    assert_eq!(original.len(), 7);
    executor
        .create_snapshot::<Counter>(&stream, StreamVersion::new(4))
        .await?;
    assert!(
        executor
            .verify_snapshot::<Counter>(&stream, StreamVersion::new(4))
            .await?
    );
    let executed = api.command(test, "snapshot-command", 16).await?;
    assert_rebuild(&reports, SnapshotStatus::Hit, 4, 7, 3);
    assert_eq!(
        reports.recent().last().unwrap().operation_id.as_deref(),
        executed["operationId"].as_str()
    );
    let committed = old_store.load(&stream).await?;
    assert_eq!(committed.len(), 8);
    assert_eq!(
        &committed[..7],
        original.as_slice(),
        "snapshot reads preserve every original event"
    );

    let observation_link = href(&catalog["observation"][0], "listHref")?;
    let window = api.json(Method::GET, observation_link, None).await?;
    assert_eq!(window["status"], "live");
    let flow = window["items"]
        .as_array()
        .ok_or("flows")?
        .iter()
        .find(|item| item["correlationId"] == executed["correlationId"])
        .ok_or("command observation")?;
    let flow = api
        .json(Method::GET, href(flow, "detailHref")?, None)
        .await?;
    // Observe retains domain/integration events, not the originating command.
    // The missing parent makes this rolling window grouped, unlike operation capture.
    assert_eq!(flow["fidelity"], "grouped");
    assert_eq!(flow["partial"], true);
    assert_eq!(flow["conflicted"], false);
    let observed_messages = flow["messageSeries"]["messages"]
        .as_array()
        .ok_or("flow messages")?;
    assert_eq!(observed_messages.len(), 2);
    let observed_domain = observed_messages
        .iter()
        .find(|m| m["kind"] == "domain-event")
        .ok_or("observed domain")?;
    let observed_integration = observed_messages
        .iter()
        .find(|m| m["kind"] == "integration-event")
        .ok_or("observed integration")?;
    assert_eq!(
        observed_domain["messageId"],
        committed.last().unwrap().event_id().as_str()
    );
    assert_eq!(
        observed_integration["causationId"],
        observed_domain["messageId"]
    );
    let before = serde_json::to_value(feed.snapshot())?;
    let observed = reports.observed();
    simulate(&api, simulation, 21, 8).await?;
    assert_eq!(reports.observed(), observed.saturating_add(1));
    assert_rebuild(&reports, SnapshotStatus::Hit, 4, 8, 4);
    assert_eq!(
        old_store.load(&stream).await?,
        committed,
        "simulation cannot commit its proposal"
    );
    assert_eq!(
        serde_json::to_value(feed.snapshot())?,
        before,
        "snapshot replay creates no observed traffic"
    );

    executor
        .create_snapshot::<Counter>(&stream, StreamVersion::new(8))
        .await?;
    simulate(&api, simulation, 21, 8).await?;
    assert_rebuild(&reports, SnapshotStatus::Hit, 8, 8, 0);

    // Retain the KV cache deliberately: recreation must fence old checkpoints,
    // even with the same aggregate ID, fixture length and shared codec registry.
    let reset = href(&catalog["testScenario"], "fixtureResetHrefTemplate")?
        .replace("{fixtureId}", "six-tens");
    let reset_response = api
        .client
        .post(api.url(&reset)?)
        .bearer_auth(TOKEN)
        .send()
        .await?;
    assert_eq!(reset_response.status(), reqwest::StatusCode::NO_CONTENT);
    let current = host.runtime().store();
    assert_ne!(current.history_authority(), Some(old_authority.as_str()));
    assert!(
        executor.rehydrate::<Counter>(&stream).await.is_err(),
        "snapshot replay through the old incarnation is fenced"
    );
    assert_ne!(
        feed.snapshot().generation,
        window["generation"].as_str().unwrap()
    );
    simulate(&api, simulation, 65, 6).await?;
    assert_rebuild(&reports, SnapshotStatus::Miss, 0, 6, 6);
    assert_eq!(
        reports.recent().last().unwrap().authority.as_deref(),
        current.history_authority()
    );

    let fresh = CommandExecutor::with_codecs(current.clone(), codecs);
    fresh
        .create_snapshot::<Counter>(&stream, StreamVersion::new(4))
        .await?;
    simulate(&api, simulation, 65, 6).await?;
    assert_rebuild(&reports, SnapshotStatus::Hit, 4, 6, 2);
    api.command(test, "snapshot-command", 65).await?;
    assert_rebuild(&reports, SnapshotStatus::Hit, 4, 6, 2);
    assert_eq!(current.load(&stream).await?.len(), 7);
    assert_eq!(*fresh.rehydrate::<Counter>(&stream).await?.state(), 65);

    host.shutdown().await?;
    connection.drain().await?;
    Ok(())
}
