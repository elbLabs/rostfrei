#![allow(
    clippy::panic_in_result_fn,
    reason = "lifecycle assertions report failures while setup errors use Result"
)]

use std::{error::Error, sync::Arc, time::Duration};

use async_trait::async_trait;
use bike_rental::{
    demo::demo_fixture,
    rental_fleet::{RentBicycle, RentBicycleHandler, RentalFleetAggregate},
};
use rostfrei::{ApplicationName, Command, CommandHandler, OperationId};
use rostfrei_nats::{NatsConnection, NatsConnectionConfig, connect};
use rostfrei_tracer::{
    ApplicationBindings, CommandInvocation, CommandOutcome, CommandPublication, CommandTransport,
    CommandTransportErrorKind, CommandTransportObserver, NatsApplication, NatsRuntime,
    NatsRuntimeConfig, TracerConfig, command_execution_fingerprint, http::HttpConfig,
};
use serde_json::json;
use tokio::sync::Notify;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
type Pause = Arc<(Notify, Notify)>;

struct PausedRent(Pause);
#[async_trait]
impl CommandHandler<RentBicycle> for PausedRent {
    type Rejection = <RentBicycleHandler as CommandHandler<RentBicycle>>::Rejection;
    async fn handle(
        &self,
        command: &RentBicycle,
        execution: &mut rostfrei::CommandExecution<'_>,
    ) -> rostfrei::CommandHandlingResult<Self::Rejection> {
        self.0.0.notify_one();
        self.0.1.notified().await;
        RentBicycleHandler.handle(command, execution).await
    }
}

struct Observer;
#[async_trait]
impl CommandTransportObserver for Observer {
    async fn command_published(&self, _: CommandPublication) {}
}

fn invocation(id: &str) -> TestResult<CommandInvocation> {
    let payload = json!({"fleet_id":"city-fleet", "bicycle_id":"bike-42"});
    Ok(CommandInvocation::new(
        OperationId::new(id)?,
        id,
        command_execution_fingerprint(
            "bike-rental",
            "rent-bicycle",
            RentBicycle::SCHEMA_VERSION,
            &payload,
        ),
        "bike-rental",
        "rent-bicycle",
        RentBicycle::SCHEMA_VERSION,
        payload,
    ))
}

async fn setup() -> TestResult<(
    NatsConnection,
    rostfrei::BoundedContext,
    Arc<ApplicationBindings>,
    Pause,
)> {
    let connection = connect(&NatsConnectionConfig::new(
        "host-admission",
        rostfrei_testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let context =
        ApplicationName::new(format!("admission-{suffix}"))?.test_bounded_context("bike-rental")?;
    let pause = Arc::new((Notify::new(), Notify::new()));
    let mut bindings = ApplicationBindings::new();
    bindings.register_aggregate::<RentalFleetAggregate>()?;
    bindings.register_command::<RentBicycle, _>(
        PausedRent(pause.clone()),
        rostfrei::JsonDomainRejectionMapper::new(
            rostfrei::CommandRejectionClassification::Conflict,
        ),
    )?;
    Ok((connection, context, Arc::new(bindings), pause))
}

async fn reject_without_publication(
    transport: &dyn CommandTransport,
    connection: &NatsConnection,
    stream: &str,
) -> TestResult {
    let mut stream = connection.jetstream().get_stream(stream).await?;
    let sequence = stream.info().await?.state.last_sequence;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        transport.invoke(invocation("after-teardown")?, Arc::new(Observer)),
    )
    .await;
    assert!(
        result.is_ok(),
        "new submission waited for a response after teardown began"
    );
    assert_eq!(
        result?
            .err()
            .ok_or("teardown must reject new submissions")?
            .kind(),
        CommandTransportErrorKind::Unavailable
    );
    assert_eq!(
        stream.info().await?.state.last_sequence,
        sequence,
        "rejected submission reached the durable command stream"
    );
    Ok(())
}

#[tokio::test]
async fn dropping_raw_runtime_closes_retained_transport_without_publishing() -> TestResult {
    let (connection, context, bindings, _) = setup().await?;
    let config = NatsRuntimeConfig::new(context, &bindings, 1)?;
    let stream = config.messaging.topology().command_stream().to_string();
    let runtime = NatsRuntime::provision(connection.clone(), config, bindings).await?;
    runtime.start_workers().await?;
    let transport = runtime.transport();
    drop(runtime);
    reject_without_publication(transport.as_ref(), &connection, &stream).await?;
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn explicit_stop_closes_admission_before_waiting_for_admitted_commands() -> TestResult {
    let (connection, context, bindings, pause) = setup().await?;
    let runtime = NatsRuntime::provision(
        connection.clone(),
        NatsRuntimeConfig::new(context, &bindings, 1)?,
        bindings,
    )
    .await?;
    runtime.apply_fixture(&demo_fixture()?).await?;
    runtime.start_workers().await?;
    let transport = runtime.transport();
    let admitted_transport = transport.clone();
    let admitted = tokio::spawn(async move {
        admitted_transport
            .invoke(
                invocation("admitted").expect("invocation"),
                Arc::new(Observer),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), pause.0.notified()).await?;
    let stop = runtime.stop_workers();
    tokio::pin!(stop);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut stop)
            .await
            .is_err(),
        "stop must wait for admitted work"
    );
    let rejection = reject_without_publication(
        transport.as_ref(),
        &connection,
        runtime
            .config()
            .messaging
            .topology()
            .command_stream()
            .as_str(),
    )
    .await;
    pause.1.notify_one();
    assert!(matches!(
        admitted.await??.outcome(),
        CommandOutcome::Accepted
    ));
    stop.await;
    rejection?;
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn host_shutdown_and_drop_close_retained_transport_at_initiation() -> TestResult {
    for mode in ["shutdown", "drop", "signal"] {
        let explicit = mode != "drop";
        let (connection, context, bindings, pause) = setup().await?;
        let normal = Arc::new(
            NatsRuntime::provision(
                connection.clone(),
                NatsRuntimeConfig::new(
                    context.application().bounded_context("bike-rental")?,
                    &bindings,
                    1,
                )?,
                bindings.clone(),
            )
            .await?,
        );
        let config = TracerConfig::new(
            tokio::net::TcpListener::bind("127.0.0.1:0").await?,
            HttpConfig::new("control")?,
        )
        .with_dispatch_runtime(normal.clone());
        let host = NatsApplication::builder(context, connection.clone(), bindings)?
            .with_default_test_fixture(demo_fixture()?)
            .with_tracer(Some(config))
            .provision_and_start()
            .await?;
        let runtime = host.runtime().clone();
        let transport = runtime.transport();
        let admitted_transport = transport.clone();
        let admitted = tokio::spawn(async move {
            admitted_transport
                .invoke(
                    invocation("admitted").expect("invocation"),
                    Arc::new(Observer),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), pause.0.notified()).await?;
        let shutdown = async move {
            match mode {
                "shutdown" => host.shutdown().await,
                "signal" => host.run_until(async {}).await,
                _ => {
                    drop(host);
                    Ok(())
                }
            }
        };
        tokio::pin!(shutdown);
        if explicit {
            assert!(
                tokio::time::timeout(Duration::from_millis(100), &mut shutdown)
                    .await
                    .is_err()
            );
        } else {
            shutdown.as_mut().await?;
        }
        let rejection = reject_without_publication(
            transport.as_ref(),
            &connection,
            runtime
                .config()
                .messaging
                .topology()
                .command_stream()
                .as_str(),
        )
        .await;
        let normal_rejection = reject_without_publication(
            normal.transport().as_ref(),
            &connection,
            normal
                .config()
                .messaging
                .topology()
                .command_stream()
                .as_str(),
        )
        .await;
        pause.1.notify_one();
        assert!(matches!(
            admitted.await??.outcome(),
            CommandOutcome::Accepted
        ));
        if explicit {
            shutdown.await?;
        }
        runtime.stop_workers().await;
        rejection?;
        normal_rejection?;
        connection.drain().await?;
    }
    Ok(())
}
