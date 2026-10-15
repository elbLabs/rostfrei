#![allow(dead_code)]
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};
use serde::{Deserialize, Serialize};
rostfrei::install_macro_support!();

#[derive(rostfrei::BoundedContext)]
#[domain(id = "counter", label = "Counter")]
struct CounterContext;
#[derive(rostfrei::DomainIdentity)]
struct CounterId(String);
#[derive(rostfrei::Entity)]
#[domain(id = "counter", label = "Counter")]
struct Counter {
    id: CounterId,
    value: u32,
}
impl rostfrei::EntityDefinition for Counter {
    type Owner = CounterAggregate;
    type Identity = CounterId;
    fn identity(&self) -> &CounterId {
        &self.id
    }
}
#[derive(rostfrei::Aggregate)]
#[domain(id = "counter", label = "Counter")]
struct CounterAggregate;
impl rostfrei::AggregateDefinition for CounterAggregate {
    type Context = CounterContext;
    type Root = Counter;
    type Event = CounterEvents;
}
#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "incremented", label = "Incremented")]
struct Incremented {
    value: u32,
}
#[derive(rostfrei::AggregateEvents)]
enum CounterEvents {
    Incremented(Incremented),
}
impl rostfrei::Initialize<CounterAggregate> for Counter {
    fn initialize(stream: &rostfrei::StreamId) -> Self {
        Self {
            id: CounterId(stream.aggregate_id().as_str().into()),
            value: 0,
        }
    }
}
impl rostfrei::Apply<Incremented> for Counter {
    fn apply(&mut self, event: &Incremented) {
        self.value = event.value;
    }
}
#[derive(rostfrei::Command)]
#[domain(id = "increment", label = "Increment", context = CounterContext)]
struct Increment {
    id: String,
}
#[derive(rostfrei::DomainError)]
#[domain(
    id = "rejected",
    label = "Rejected",
    code = "REJECTED",
    message = "Rejected"
)]
struct Rejected;
type Pause = std::sync::Arc<(tokio::sync::Notify, tokio::sync::Notify)>;
struct IncrementHandler {
    step: u32,
    pause: Option<Pause>,
}
#[async_trait]
impl CommandHandler<Increment> for IncrementHandler {
    type Rejection = Rejected;
    async fn handle(
        &self,
        command: &Increment,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Rejected> {
        if command.id == "blocked" {
            if let Some(pause) = &self.pause {
                pause.0.notify_one();
                pause.1.notified().await;
            }
        }
        let mut counter = execution.load::<CounterAggregate>(&command.id).await?;
        let value = counter.state().value + self.step;
        counter.aggregate_mut().raise(Incremented { value });
        Ok(CommandDecision::Accepted)
    }
}

pub fn bindings()
-> Result<std::sync::Arc<rostfrei_tracer::ApplicationBindings>, Box<dyn std::error::Error>> {
    bindings_with_pause(None)
}
fn bindings_with_pause(
    pause: Option<Pause>,
) -> Result<std::sync::Arc<rostfrei_tracer::ApplicationBindings>, Box<dyn std::error::Error>> {
    let mut bindings = rostfrei_tracer::ApplicationBindings::new();
    bindings.register_aggregate::<CounterAggregate>()?;
    bindings.register_command::<Increment, _>(
        IncrementHandler { step: 2, pause },
        rostfrei::JsonDomainRejectionMapper::new(
            rostfrei::CommandRejectionClassification::Conflict,
        ),
    )?;
    Ok(std::sync::Arc::new(bindings))
}

#[tokio::test]
async fn standalone_host_discovers_and_executes_behavior_through_real_nats()
-> Result<(), Box<dyn std::error::Error>> {
    use rostfrei::ApplicationName;
    use rostfrei_nats::{NatsConnectionConfig, connect};
    use rostfrei_tracer::{
        ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository, NatsApplication,
        TracerConfig, http::HttpConfig,
    };
    use std::sync::Arc;
    let connection = connect(&NatsConnectionConfig::new(
        "external-host",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new(format!("consumer-{}", std::process::id()))?;
    let fixture = serde_json::from_str(include_str!("../fixture.json"))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let feed = rostfrei_tracer::ObservationFeed::new(
        application.clone(), rostfrei_tracer::ObservationScope::Test,
    );
    let config = TracerConfig::new(listener, HttpConfig::new("external-consumer-token")?)
        .with_continuous_observation(feed.clone())
        .with_test_repository(Arc::new(FilesystemTestRepository::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests"
        ))?))
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    let host = NatsApplication::builder(
        application.test_bounded_context("counter")?,
        connection.clone(),
        bindings()?,
    )?
    .with_default_test_fixture(fixture)
    .with_tracer(Some(config))
    .provision_and_start()
    .await?;
    let client = reqwest::Client::new();
    let get = |path: &str| {
        client
            .get(format!("http://{address}{path}"))
            .bearer_auth("external-consumer-token")
    };
    let catalog: serde_json::Value = get("/catalog")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(catalog["catalogVersion"], 1);
    assert_eq!(catalog["contexts"][0]["commands"][0]["id"], "increment");
    assert!(host.runtime().has_correlation_observer().await);
    assert_eq!(feed.snapshot().status, rostfrei_tracer::ObservationStatus::Live);
    assert_eq!(catalog["observation"][0]["scope"], "test");
    let definitions: serde_json::Value = get(catalog["behavioralTest"]["definitionsHref"]
        .as_str()
        .unwrap())
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    let run = definitions["items"][0]["runHref"].as_str().unwrap();
    let mut events_href = String::new();
    for _ in 0..2 {
        let report: serde_json::Value = client
            .post(format!("http://{address}{run}"))
            .bearer_auth("external-consumer-token")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(report["status"], "passed", "{report:#}");
        assert_eq!(report["comparison"]["status"], "passed", "{report:#}");
        assert_eq!(feed.snapshot().status, rostfrei_tracer::ObservationStatus::Live);
        events_href = report["correlationEventsHref"].as_str().unwrap().to_owned();
        let stream = rostfrei::StreamId::new(
            rostfrei::StreamAggregateType::new("counter/counter")?,
            rostfrei::StreamAggregateId::new("one")?,
        );
        let history = host.runtime().store().load(&stream).await?;
        assert_eq!(history.len(), 2);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(history[1].payload())?["value"],
            7
        );
        assert!(history[1].causation_id().is_some());
        assert!(history[1].correlation_id().is_some());
        let event = &report["observed"]["messages"][1];
        assert_eq!(event["messageId"], history[1].event_id().as_str());
        assert_eq!(
            event["causationId"],
            history[1].causation_id().unwrap().as_str()
        );
        assert_eq!(
            event["correlationId"],
            history[1].correlation_id().unwrap().as_str()
        );
    }
    // Preview executes the same injected handler without committing anything.
    let preview = catalog["contexts"][0]["commands"][0]["versions"][0]["simulateHref"]
        .as_str()
        .or_else(|| {
            catalog["contexts"][0]["commands"][0]["versions"][0]["simulateHrefTemplate"].as_str()
        })
        .unwrap();
    let response = client
        .post(format!("http://{address}{preview}"))
        .bearer_auth("external-consumer-token")
        .json(&serde_json::json!({"schemaVersion":1,"payload":{"id":"one"}}))
        .send()
        .await?
        .error_for_status()?;
    let location = response
        .headers()
        .get("location")
        .unwrap()
        .to_str()?
        .to_owned();
    let mut completed = false;
    for _ in 0..100 {
        let operation: serde_json::Value = get(&location)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if operation["status"] == "completed" {
            assert_eq!(operation["result"]["decision"], "accepted");
            completed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(completed, "Preview did not complete");
    let stream = rostfrei::StreamId::new(
        rostfrei::StreamAggregateType::new("counter/counter")?,
        rostfrei::StreamAggregateId::new("one")?,
    );
    assert_eq!(host.runtime().store().load(&stream).await?.len(), 2);
    let events = get(&events_href).send().await?.error_for_status()?;
    let runtime = host.runtime().clone();
    tokio::time::timeout(std::time::Duration::from_secs(5), host.shutdown()).await??;
    tokio::time::timeout(std::time::Duration::from_secs(2), events.bytes()).await??;
    assert!(!runtime.has_correlation_observer().await);
    assert_eq!(feed.snapshot().status, rostfrei_tracer::ObservationStatus::Unavailable);
    assert!(get("/catalog").send().await.is_err());
    connection.drain().await?;
    Ok(())
}

struct Observer;
#[async_trait]
impl rostfrei_tracer::CommandTransportObserver for Observer {
    async fn command_published(&self, _: rostfrei_tracer::CommandPublication) {}
}

async fn invoke(
    runtime: &rostfrei_tracer::NatsRuntime,
    operation: &str,
) -> Result<rostfrei_tracer::CommandReceipt, rostfrei_tracer::CommandTransportError> {
    invoke_id(runtime, operation, "one").await
}
async fn invoke_id(
    runtime: &rostfrei_tracer::NatsRuntime,
    operation: &str,
    id: &str,
) -> Result<rostfrei_tracer::CommandReceipt, rostfrei_tracer::CommandTransportError> {
    let payload = serde_json::json!({"id":id});
    let fingerprint =
        rostfrei_tracer::command_execution_fingerprint("counter", "increment", 1, &payload);
    runtime
        .transport()
        .invoke(
            rostfrei_tracer::CommandInvocation::new(
                rostfrei::OperationId::new(operation).unwrap(),
                operation,
                fingerprint,
                "counter",
                "increment",
                1,
                payload,
            ),
            std::sync::Arc::new(Observer),
        )
        .await
}

#[tokio::test]
async fn disabled_commands_work_and_failed_test_reset_fails_closed_without_touching_normal()
-> Result<(), Box<dyn std::error::Error>> {
    use rostfrei::ApplicationName;
    use rostfrei_nats::{NatsConnectionConfig, connect};
    use rostfrei_tracer::{NatsApplication, TestScenarioReset};
    let connection = connect(&NatsConnectionConfig::new(
        "external-lifecycle",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new(format!("lifecycle-{}", std::process::id()))?;
    let bindings = bindings()?;
    let normal = NatsApplication::builder(
        application.bounded_context("counter")?,
        connection.clone(),
        bindings.clone(),
    )?
    .with_tracer(None)
    .provision_and_start()
    .await?;
    assert!(normal.tracer().is_none());
    assert!(!normal.runtime().has_correlation_observer().await);
    assert!(matches!(
        invoke(normal.runtime(), "normal").await?.outcome(),
        rostfrei_tracer::CommandOutcome::Accepted
    ));
    let fixture = serde_json::from_str(include_str!("../fixture.json"))?;
    let test = NatsApplication::builder(
        application.test_bounded_context("counter")?,
        connection.clone(),
        bindings,
    )?
    .with_default_test_fixture(fixture)
    .provision_and_start()
    .await?;
    assert!(matches!(
        invoke(test.runtime(), "reusable").await?.outcome(),
        rostfrei_tracer::CommandOutcome::Accepted
    ));
    let mut invalid: serde_json::Value = serde_json::from_str(include_str!("../fixture.json"))?;
    invalid["messages"][0]["aggregate"]["type"] = "unknown/aggregate".into();
    assert!(
        test.runtime()
            .reset(&serde_json::from_value(invalid)?)
            .await
            .is_err()
    );
    assert!(
        test.runtime().start_workers().await.is_err(),
        "worker restart must not bypass a failed fixture reset"
    );
    assert_eq!(
        invoke(test.runtime(), "closed").await.unwrap_err().kind(),
        rostfrei_tracer::CommandTransportErrorKind::Unavailable
    );
    test.runtime()
        .reset(&serde_json::from_str(include_str!("../fixture.json"))?)
        .await?;
    assert!(!invoke(test.runtime(), "reusable").await?.duplicate());
    let stream = rostfrei::StreamId::new(
        rostfrei::StreamAggregateType::new("counter/counter")?,
        rostfrei::StreamAggregateId::new("one")?,
    );
    assert_eq!(normal.runtime().store().load(&stream).await?.len(), 1);
    test.shutdown().await?;
    assert!(matches!(
        invoke(normal.runtime(), "still-normal").await?.outcome(),
        rostfrei_tracer::CommandOutcome::Accepted
    ));
    normal.shutdown().await?;
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn reset_waits_for_active_submissions() -> Result<(), Box<dyn std::error::Error>> {
    use rostfrei_tracer::{NatsApplication, TestScenarioReset};
    use std::{sync::Arc, time::Duration};
    let connection = rostfrei_nats::connect(&rostfrei_nats::NatsConnectionConfig::new(
        "reset-gate",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let context = rostfrei::ApplicationName::new(format!("reset-gate-{}", std::process::id()))?
        .test_bounded_context("counter")?;
    let pause = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
    let host = NatsApplication::builder(
        context,
        connection.clone(),
        bindings_with_pause(Some(pause.clone()))?,
    )?
    .with_default_test_fixture(serde_json::from_str(include_str!("../fixture.json"))?)
    .provision_and_start()
    .await?;
    let runtime = host.runtime().clone();
    let active = tokio::spawn(async move { invoke_id(&runtime, "active", "blocked").await });
    tokio::time::timeout(Duration::from_secs(5), pause.0.notified()).await?;
    let runtime = host.runtime().clone();
    let mut reset = tokio::spawn(async move {
        runtime
            .reset(&serde_json::from_str(include_str!("../fixture.json")).unwrap())
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut reset)
            .await
            .is_err()
    );
    pause.1.notify_one();
    assert!(matches!(
        active.await??.outcome(),
        rostfrei_tracer::CommandOutcome::Accepted
    ));
    reset.await??;
    let blocked = rostfrei::StreamId::new(
        rostfrei::StreamAggregateType::new("counter/counter")?,
        rostfrei::StreamAggregateId::new("blocked")?,
    );
    assert!(host.runtime().store().load(&blocked).await?.is_empty());
    host.shutdown().await?;
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn worker_startup_failure_and_unexpected_exit_are_surfaced()
-> Result<(), Box<dyn std::error::Error>> {
    use rostfrei_tracer::{NatsApplication, NatsRuntimeConfig};
    use std::time::Duration;
    let connection = rostfrei_nats::connect(&rostfrei_nats::NatsConnectionConfig::new(
        "worker-failure",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let context = rostfrei::ApplicationName::new(format!("worker-failure-{}", std::process::id()))?
        .bounded_context("counter")?;
    let bindings = bindings()?;
    let config = NatsRuntimeConfig::new(context.clone(), &bindings, 1)?;
    config.provision(&connection).await?;
    let stream = connection
        .jetstream()
        .get_stream(config.messaging.topology().command_stream().as_str())
        .await?;
    let durable = config.commands[0].durable_name().as_str();
    stream.delete_consumer(durable).await?;
    assert!(
        NatsApplication::builder(context.clone(), connection.clone(), bindings.clone())?
            .connect_and_start()
            .await
            .is_err()
    );
    let host = NatsApplication::builder(context, connection.clone(), bindings)?
        .provision_and_start()
        .await?;
    let runtime = host.runtime().clone();
    assert!(matches!(
        invoke(&runtime, "before-exit").await?.outcome(),
        rostfrei_tracer::CommandOutcome::Accepted
    ));
    stream.delete_consumer(durable).await?;
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        host.run_until(std::future::pending()),
    )
    .await?;
    assert!(result.is_err());
    assert_eq!(
        invoke(&runtime, "after-exit").await.unwrap_err().kind(),
        rostfrei_tracer::CommandTransportErrorKind::Unavailable
    );
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn observer_startup_failure_stops_previously_active_workers()
-> Result<(), Box<dyn std::error::Error>> {
    use rostfrei_tracer::{NatsApplication, OperationMode};
    use std::sync::Arc;
    let connection = rostfrei_nats::connect(&rostfrei_nats::NatsConnectionConfig::new(
        "observer-failure",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let context =
        rostfrei::ApplicationName::new(format!("observer-failure-{}", std::process::id()))?
            .test_bounded_context("counter")?;
    let bindings = bindings()?;
    let host = NatsApplication::builder(context, connection.clone(), bindings.clone())?
        .with_default_test_fixture(serde_json::from_str(include_str!("../fixture.json"))?)
        .provision_and_start()
        .await?;
    assert!(matches!(
        invoke(host.runtime(), "before-observer").await?.outcome(),
        rostfrei_tracer::CommandOutcome::Accepted
    ));
    let tracer = bindings
        .tracer_builder(Arc::new(host.runtime().store().clone()))
        .build()?;
    connection
        .delete_stream_if_exists(
            host.runtime()
                .config()
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .await?;
    assert!(
        host.runtime()
            .attach_observer(tracer.correlation_observer(OperationMode::Test))
            .await
            .is_err()
    );
    assert_eq!(
        invoke(host.runtime(), "after-observer-failure")
            .await
            .unwrap_err()
            .kind(),
        rostfrei_tracer::CommandTransportErrorKind::Unavailable
    );
    host.shutdown().await?;
    connection.drain().await?;
    Ok(())
}
