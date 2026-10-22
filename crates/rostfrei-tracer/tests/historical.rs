//! Historical reads must survive a fresh Tracer and leave the real NATS pipeline untouched.
#![cfg(feature = "nats-host")]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic_in_result_fn
)]

use async_trait::async_trait;
use http_body_util::BodyExt;
use rostfrei::{
    Aggregate, CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult,
};
use rostfrei_tracer::{
    ApplicationBindings, ExposeTracePayloadsForLocalDevelopment, NatsApplication, TracerConfig,
    http::{self, HttpConfig},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{error::Error, sync::Arc};
use tower::ServiceExt;

rostfrei::install_macro_support!();
type Result<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(rostfrei::BoundedContext)]
#[domain(id = "archive", label = "Archive")]
struct Context;
#[derive(rostfrei::Command)]
#[domain(context = Context, id = "record", label = "Record")]
struct Record {
    id: String,
}
#[derive(Serialize, Deserialize, rostfrei::DomainEvent)]
#[domain(id = "recorded", label = "Recorded")]
struct Recorded {
    value: u64,
}
#[derive(rostfrei::AggregateEvents)]
enum Events {
    Recorded(Recorded),
}
struct Counter;
impl Aggregate for Counter {
    type State = u64;
    type Event = Events;
    const BOUNDED_CONTEXT: &'static str = "archive";
    const AGGREGATE_TYPE: &'static str = "archive/counter";
    fn initial(_: &rostfrei::StreamId) -> u64 {
        0
    }
    fn apply(state: &mut u64, event: &Events) {
        let Events::Recorded(event) = event;
        *state = event.value;
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
struct Handler;
#[async_trait]
impl CommandHandler<Record> for Handler {
    type Rejection = Refused;
    async fn handle(
        &self,
        command: &Record,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Refused> {
        let mut counter = execution.load::<Counter>(&command.id).await?;
        let value = counter.state().saturating_add(1);
        counter.raise(Events::Recorded(Recorded { value }));
        Ok(CommandDecision::Accepted)
    }
}
#[derive(Serialize, Deserialize)]
struct Published {
    value: u64,
}
impl rostfrei::IntegrationEvent for Published {
    const EVENT_NAME: &'static str = "published";
    const SCHEMA_VERSION: u32 = 1;
}
struct Publish;
impl rostfrei::IntegrationEventMapper<Recorded> for Publish {
    type Output = Published;
    fn map(&self, event: &rostfrei::CommittedDomainEvent<'_, Recorded>) -> Published {
        Published {
            value: event.event().value,
        }
    }
}

async fn get(app: &axum::Router, href: &str, token: &str) -> Result<(u16, Value)> {
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
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&bytes)?))
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn persisted_history_and_flows_survive_tracer_recreation() -> Result {
    let connection = rostfrei_nats::connect(&rostfrei_nats::NatsConnectionConfig::new(
        "historical-test",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = rostfrei::ApplicationName::new(format!("historical-{}", std::process::id()))?;
    let mut bindings = ApplicationBindings::new();
    bindings.register_aggregate::<Counter>()?;
    bindings.register_command::<Record, _>(
        Handler,
        rostfrei::JsonDomainRejectionMapper::new(
            rostfrei::CommandRejectionClassification::Conflict,
        ),
    )?;
    bindings.register_integration_event::<Counter, Recorded, _>(Publish);
    let bindings = Arc::new(bindings);
    let normal_config = rostfrei_tracer::NatsRuntimeConfig::new(
        application.bounded_context("archive")?,
        &bindings,
        1,
    )?
    .with_fresh_completed_outcome_epoch("historical")?;
    let normal = Arc::new(
        rostfrei_tracer::NatsRuntime::provision(
            connection.clone(),
            normal_config,
            bindings.clone(),
        )
        .await?,
    );
    normal.apply_fixture(&serde_json::from_value(json!({
        "schemaVersion":1,"id":"normal","revision":"1","messages":[{
            "kind":"domain-event","messageId":"normal-event","correlationId":"normal-flow",
            "name":"recorded","schemaVersion":1,"aggregate":{"type":"archive/counter","id":"one / +"},
            "streamVersion":1,"payload":{"value":7}
        }]
    }))?).await?;
    let fixture = serde_json::from_value(
        json!({"schemaVersion":1,"id":"empty","revision":"1","messages":[]}),
    )?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let host = NatsApplication::builder(
        application.test_bounded_context("archive")?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_completed_outcome_epoch("historical")?
    .with_default_test_fixture(fixture)
    .with_tracer(Some(
        TracerConfig::new(listener, HttpConfig::new("control")?)
            .with_dispatch_runtime(normal.clone())
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment)),
    ))
    .provision_and_start()
    .await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    // Real JSON-v2 behavior acceptance: fixture reset + transported handler + domain/integration workers.
    let report: Value = client.post(format!("{origin}/test-runs")).bearer_auth("control")
        .json(&json!({"schemaVersion":2,"id":"record","name":"Record history",
            "setup":{"fixture":"empty"},"expected":{"within":"10s","settleFor":"100ms","graphs":[{"nodes":[
                {"kind":"command","key":"root","context":"archive","name":"record","schemaVersion":1,"payload":{"id":"one / +"},"outcome":"accepted"},
                {"kind":"domain-event","key":"event","parentKey":"root","name":"recorded","schemaVersion":1,"payload":{"value":1}},
                {"kind":"integration-event","key":"published","parentKey":"event","name":"published","schemaVersion":1,"payload":{"value":1}}
            ]}]}})).send().await?.error_for_status()?.json().await?;
    assert_eq!(report["status"], "passed", "{report:#}");
    let correlation = report["observed"]["messages"][0]["correlationId"]
        .as_str()
        .unwrap();
    // Fresh operation/observation state: only persisted storage can answer these reads.
    let tracer = host
        .runtime()
        .tracer_builder()
        .with_historical_reader(normal.historical_reader())
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
        .build()?;
    let app = http::router(
        tracer,
        HttpConfig::new("control")?
            .with_inspection_token("inspection")?
            .with_dispatch_token("dispatch")?,
    );
    let (_, catalog) = get(&app, "/catalog", "control").await?;
    assert_eq!(
        catalog["historical"][0]["scope"], "test",
        "Historical must advertise persisted inspection"
    );
    let href = catalog["historical"][0]["messagesHref"].as_str().unwrap();
    assert_eq!(catalog["historical"].as_array().unwrap().len(), 1);
    let (_, inspection) = get(&app, "/catalog", "inspection").await?;
    assert_eq!(inspection["historical"].as_array().unwrap().len(), 1);
    assert_eq!(inspection["historical"][0]["scope"], "production");
    let production_href = inspection["historical"][0]["messagesHref"]
        .as_str()
        .unwrap();
    assert_eq!(get(&app, production_href, "control").await?.0, 403);
    assert_eq!(get(&app, production_href, "dispatch").await?.0, 403);
    let (status, production) = get(&app, production_href, "inspection").await?;
    assert_eq!(status, 200);
    assert_eq!(production["items"][0]["payload"]["value"], 7);
    let before = stored_state(&connection, host.runtime()).await?;
    let (status, events) = get(
        &app,
        &format!("{href}?aggregateType=archive%2Fcounter&aggregateId=one%20%2F%20%2B&limit=1"),
        "control",
    )
    .await?;
    assert_eq!(status, 200, "{events:#}");
    assert_eq!(events["items"][0]["name"], "recorded");
    assert_eq!(events["items"][0]["streamVersion"], "1");
    assert_eq!(events["items"][0]["payload"]["value"], 1);
    let query = format!("{href}?correlationId={correlation}&limit=1");
    let mut next = Some(query);
    let mut messages = Vec::new();
    let mut first_next = None;
    while let Some(href) = next.take() {
        let (status, page) = get(&app, &href, "control").await?;
        assert_eq!(status, 200, "{page:#}");
        messages.extend(page["items"].as_array().unwrap().iter().cloned());
        next = page["nextHref"].as_str().map(str::to_owned);
        if first_next.is_none() {
            first_next.clone_from(&next);
        }
        assert!(messages.len() < 10, "pagination must advance");
    }
    let domain = messages
        .iter()
        .find(|m| m["kind"] == "domain-event")
        .unwrap();
    let integration = messages
        .iter()
        .find(|m| m["kind"] == "integration-event")
        .unwrap();
    let command = messages.iter().find(|m| m["kind"] == "command").unwrap();
    assert_eq!(domain["causationId"], command["messageId"]);
    assert_eq!(integration["causationId"], domain["messageId"]);
    assert_eq!(command["evidence"], "command-response");
    assert_eq!(command["response"]["status"], "accepted");
    assert!(
        command.get("payload").is_none(),
        "acknowledged command payloads are not invented"
    );
    assert_eq!(get(&app, href, "inspection").await?.0, 403);
    assert_eq!(get(&app, href, "dispatch").await?.0, 403);
    assert_eq!(
        get(&app, &format!("{href}?limit=0"), "control").await?.0,
        400
    );
    let redacted = http::router(
        host.runtime().tracer_builder().build()?,
        HttpConfig::new("control")?,
    );
    let (_, page) = get(&redacted, href, "control").await?;
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m.get("payload").is_none())
    );
    assert_eq!(
        before,
        stored_state(&connection, host.runtime()).await?,
        "reads cannot publish, consume or create consumers"
    );
    for query in [
        "limit=101",
        "cursor=garbage",
        "aggregateId=one",
        "aggregateType=unknown",
        "correlationId=",
    ] {
        assert_eq!(
            get(&app, &format!("{href}?{query}"), "control").await?.0,
            400
        );
    }
    assert_eq!(
        get(
            &app,
            &first_next
                .as_ref()
                .unwrap()
                .replace("/test/", "/production/"),
            "inspection"
        )
        .await?
        .0,
        400
    );
    // Each stream's high-water mark remains fixed while more traffic is committed.
    let (_, first) = get(&app, &format!("{href}?limit=1"), "control").await?;
    let (_, time_first) = get(&app, &format!("{href}?order=time&limit=1"), "control").await?;
    let operation: Value = client
        .post(format!("{origin}/contexts/archive/commands/record/test"))
        .bearer_auth("control")
        .header("Idempotency-Key", "append-after-history-page")
        .json(&json!({"schemaVersion":1,"payload":{"id":"one / +"}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let captured: Value = client
        .get(format!(
            "{origin}{}?within=10s&settleFor=100ms",
            operation["messageSeriesHref"].as_str().unwrap()
        ))
        .bearer_auth("control")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(captured["capture"]["settled"], true);
    for boundary in [first, time_first] {
        let mut next = boundary["nextHref"].as_str().map(str::to_owned);
        while let Some(link) = next.take() {
            let (_, page) = get(&app, &link, "control").await?;
            assert!(
                page["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|m| m["correlationId"] == correlation)
            );
            next = page["nextHref"].as_str().map(str::to_owned);
        }
    }
    let (_, latest) = get(
        &app,
        &format!("{href}?aggregateType=archive%2Fcounter"),
        "control",
    )
    .await?;
    assert_eq!(latest["items"].as_array().unwrap().len(), 2);
    let (_, aggregate_time) = get(
        &app,
        &format!("{href}?order=time&aggregateType=archive%2Fcounter&aggregateId=one%20%2F%20%2B"),
        "control",
    )
    .await?;
    assert_eq!(aggregate_time["items"], latest["items"]);
    let timeline_href = catalog["historical"][0]["timelineHref"]
        .as_str()
        .expect("Historical must advertise chronological timeline replay across pages");
    let chronology_before = stored_state(&connection, host.runtime()).await?;
    let timeline_cursor = assert_timeline(&app, timeline_href).await?;
    assert_eq!(
        stored_state(&connection, host.runtime()).await?,
        chronology_before,
        "loading timeline milestones cannot publish or consume messages"
    );
    let reset = client
        .post(format!("{origin}/test-scenario/reset"))
        .bearer_auth("control")
        .send()
        .await?;
    assert_eq!(reset.status(), 204);
    assert_eq!(get(&app, &timeline_cursor, "control").await?.0, 410);
    assert_eq!(
        get(&app, first_next.as_deref().unwrap(), "control")
            .await?
            .0,
        410
    );
    assert!(
        get(&app, href, "control").await?.1["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        get(&app, production_href, "inspection").await?.1["items"][0]["payload"]["value"],
        7
    );
    // A selective scan is bounded even when no record matches; malformed records
    // produce an explicit skipped count and a continuation, never fabricated nodes.
    let address = host
        .runtime()
        .config()
        .context
        .integration_event_address("published")?;
    for _ in 0..502 {
        connection
            .jetstream()
            .publish(address.as_str().to_owned(), "invalid-json".into())
            .await?
            .await?;
    }
    for order in ["source", "time"] {
        let (_, scan) = get(
            &app,
            &format!("{href}?correlationId=missing&order={order}"),
            "control",
        )
        .await?;
        assert_eq!(scan["scannedMessages"], 500);
        assert_eq!(scan["skippedMessages"], 500);
        assert!(scan["items"].as_array().unwrap().is_empty());
        let (_, final_scan) = get(&app, scan["nextHref"].as_str().unwrap(), "control").await?;
        assert_eq!(final_scan["scannedMessages"], 2);
        assert!(final_scan.get("nextHref").is_none());
    }
    eprintln!(
        "Historical: persisted aggregate, response-backed command flow, redaction, auth and reset fencing passed"
    );
    host.shutdown().await?;
    connection.drain().await?;
    Ok(())
}

async fn assert_timeline(app: &axum::Router, href: &str) -> Result<String> {
    let (status, first) = get(app, &format!("{href}&limit=1"), "control").await?;
    assert_eq!(status, 200, "{first:#}");
    assert_eq!(first["order"], "time");
    let cursor = first["nextHref"]
        .as_str()
        .ok_or("timeline continuation")?
        .to_owned();
    let mut records = first["items"].as_array().unwrap().clone();
    let mut next = Some(cursor.clone());
    while let Some(link) = next.take() {
        let (status, page) = get(app, &link, "control").await?;
        assert_eq!(status, 200);
        assert_eq!(page["order"], "time");
        assert_eq!(page["generation"], first["generation"]);
        records.extend(page["items"].as_array().unwrap().iter().cloned());
        next = page["nextHref"].as_str().map(str::to_owned);
        assert!(
            records.len() <= 6,
            "timeline must make progress without duplicates"
        );
    }
    assert_eq!(
        records.len(),
        6,
        "both real commands' domain events, responses and integration events survive pagination"
    );
    let timestamps = records
        .iter()
        .map(|record| {
            record["storedAtUnixNanos"]
                .as_str()
                .ok_or("exact persisted timestamp")?
                .parse::<i128>()
                .map_err(Into::into)
        })
        .collect::<Result<Vec<_>>>()?;
    assert!(
        timestamps.windows(2).all(|pair| pair[0] <= pair[1]),
        "milestones must be chronological across NATS streams: {records:#?}"
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record["id"].clone())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        records.len()
    );
    for token in ["inspection", "dispatch"] {
        assert_eq!(get(app, href, token).await?.0, 403);
    }
    assert_eq!(
        get(app, &format!("{cursor}&order=time"), "control")
            .await?
            .0,
        400
    );
    eprintln!(
        "Historical timeline: six milestones across one-record pages, exact timestamps, scope authorization and read-only storage passed"
    );
    Ok(cursor)
}

async fn stored_state(
    connection: &rostfrei_nats::NatsConnection,
    runtime: &rostfrei_tracer::NatsRuntime,
) -> Result<Vec<Value>> {
    let config = runtime.config();
    let mut names: Vec<_> = config
        .messaging
        .streams()
        .iter()
        .map(|stream| stream.name().as_str().to_owned())
        .collect();
    names.push(config.event_store.stream_name().to_owned());
    let mut states = Vec::new();
    for name in names {
        let stream = connection.jetstream().get_stream(name).await?;
        let state = &stream.cached_info().state;
        states.push(json!({"messages": state.messages, "last": state.last_sequence, "consumers": state.consumer_count}));
    }
    Ok(states)
}
