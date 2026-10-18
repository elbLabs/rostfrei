#![allow(
    clippy::panic_in_result_fn,
    reason = "integration assertions report behavior failures"
)]

use async_trait::async_trait;
use bike_rental::{
    demo::{demo_fixture, demo_stream},
    tracer,
};
use rostfrei::{ApplicationName, CommittedEventContext, IntegrationEvent, IntegrationEventBus};
use rostfrei_messaging_core::{
    DeliveryDisposition, IntegrationEventAddress, MessageDelivery, MessageHandler,
};
use rostfrei_nats::{NatsConnectionConfig, connect};
use rostfrei_tracer::{
    ExposeTracePayloadsForLocalDevelopment, NatsApplication, NatsRuntime, NatsRuntimeConfig,
    ObservationFeed, ObservationScope, TracerConfig, http::HttpConfig,
};
use serde::{Deserialize, Serialize};
use std::{error::Error, sync::Arc, time::Duration};

#[derive(Deserialize, Serialize)]
struct ProducerChanged;
impl IntegrationEvent for ProducerChanged {
    const EVENT_NAME: &'static str = "producer-changed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("producer");
}

struct RentAfterChange;
impl rostfrei::IntegrationCommandMapper<ProducerChanged> for RentAfterChange {
    type Command = bike_rental::rental_fleet::RentBicycle;
    type Error = &'static str;
    fn map(&self, _: &ProducerChanged) -> Result<Self::Command, Self::Error> {
        Ok(Self::Command {
            fleet_id: bike_rental::rental_fleet::FleetId::new("city-fleet").ok_or("fleet")?,
            bicycle_id: bike_rental::rental_fleet::BicycleId::new("bike-42").ok_or("bicycle")?,
        })
    }
}

// Same local event name/schema, distinct producing context and payload codec.
#[derive(Deserialize, Serialize)]
struct OtherProducerChanged {
    note: String,
}
impl IntegrationEvent for OtherProducerChanged {
    const EVENT_NAME: &'static str = ProducerChanged::EVENT_NAME;
    const SCHEMA_VERSION: u32 = ProducerChanged::SCHEMA_VERSION;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("other-producer");
}

struct RecordIntegration(tokio::sync::mpsc::UnboundedSender<IntegrationEventAddress>);
#[async_trait]
impl MessageHandler<IntegrationEventAddress> for RecordIntegration {
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        let _ = self.0.send(delivery.address().clone());
        DeliveryDisposition::Acknowledge
    }
}

#[tokio::test]
async fn typed_handler_subscribes_to_declared_producer() -> Result<(), Box<dyn Error + Send + Sync>>
{
    let connection = connect(&NatsConnectionConfig::new(
        "producer-routing",
        rostfrei_testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("producer-routing-{suffix}"))?;
    let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
    let mut bindings = tracer::bindings()?;
    bindings.register_integration_handler::<ProducerChanged, _>(RecordIntegration(sender));
    let host = NatsApplication::builder(
        application.test_bounded_context("bike-rental")?,
        connection.clone(),
        Arc::new(bindings),
    )?
    .with_fresh_completed_outcome_epoch("producer-routing")?
    .with_default_test_fixture(demo_fixture()?)
    .provision_and_start()
    .await?;
    let producer = application
        .test_bounded_context(ProducerChanged::BOUNDED_CONTEXT.ok_or("producer context")?)?;
    let expected = producer.integration_event_address(ProducerChanged::EVENT_NAME)?;
    let actual = host
        .runtime()
        .config()
        .integrations
        .last()
        .ok_or("subscription")?
        .address();
    println!("declared producer address: {expected}; host subscription: {actual}");
    let history = host.runtime().store().load(&demo_stream()).await?;
    let source = history.first().ok_or("source fixture event")?;
    let adapter = Arc::new(
        connection.messaging_adapter(host.runtime().config().messaging.topology().clone()),
    );
    IntegrationEventBus::new(producer, adapter)
        .publish(CommittedEventContext::new(source)?, ProducerChanged)
        .await?;
    let observed = tokio::time::timeout(Duration::from_secs(2), received.recv()).await;
    host.shutdown().await?;
    connection.drain().await?;
    assert!(
        observed.is_ok(),
        "declared producer event was not delivered: {observed:?}"
    );
    assert_eq!(observed?, Some(expected));
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "typed mapping and scope isolation form one end-to-end scenario"
)]
async fn typed_mapper_dispatches_across_contexts_with_scoped_causal_evidence()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let connection = connect(&NatsConnectionConfig::new(
        "producer-mapper",
        rostfrei_testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("producer-mapper-{suffix}"))?;
    let (sender, mut other_received) = tokio::sync::mpsc::unbounded_channel();
    let mut bindings = tracer::bindings()?;
    // The application supplies only the typed business mapper: no bus, subject or durable.
    bindings.register_integration_command::<ProducerChanged, _>(RentAfterChange);
    bindings.register_integration_handler::<OtherProducerChanged, _>(RecordIntegration(sender));
    let bindings = Arc::new(bindings);
    let normal = Arc::new(
        NatsRuntime::provision(
            connection.clone(),
            NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
                .with_fresh_completed_outcome_epoch("producer-routing")?,
            bindings.clone(),
        )
        .await?,
    );
    normal.apply_fixture(&demo_fixture()?).await?;
    let feed = ObservationFeed::new(application.clone(), ObservationScope::Test);
    let tracer_config = TracerConfig::new(
        tokio::net::TcpListener::bind("127.0.0.1:0").await?,
        HttpConfig::new("control")?,
    )
    .with_dispatch_runtime(normal.clone())
    .with_continuous_observation(feed.clone())
    .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    let host = NatsApplication::builder(
        application.test_bounded_context("bike-rental")?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_completed_outcome_epoch("producer-routing")?
    .with_default_test_fixture(demo_fixture()?)
    .with_tracer(Some(tracer_config))
    .provision_and_start()
    .await?;
    let integrations = &host.runtime().config().integrations;
    let fallback = integrations.first().ok_or("fallback subscription")?;
    assert_eq!(
        fallback.address(),
        &host
            .runtime()
            .config()
            .context
            .integration_event_address(bike_rental::BICYCLE_RENTAL_STARTED_EVENT_NAME)?
    );
    let foreign: Vec<_> = integrations
        .iter()
        .filter(|c| c.address().name() == ProducerChanged::EVENT_NAME)
        .collect();
    assert_eq!(foreign.len(), 2);
    assert!(
        foreign
            .iter()
            .all(|c| c.durable_name().context() == "bike-rental"
                && c.address().context() != "bike-rental")
    );
    assert_ne!(
        foreign.first().ok_or("first")?.durable_name(),
        foreign.last().ok_or("last")?.durable_name()
    );

    for runtime in [host.runtime(), &normal] {
        let source_history = runtime.store().load(&demo_stream()).await?;
        let source = source_history.first().ok_or("source fixture")?;
        let committed = CommittedEventContext::new(source)?;
        let producer = application.bounded_context_in_scope(
            runtime.config().context.traffic_scope(),
            ProducerChanged::BOUNDED_CONTEXT.ok_or("producer")?,
        )?;
        let bus = IntegrationEventBus::new(
            producer,
            Arc::new(connection.messaging_adapter(runtime.config().messaging.topology().clone())),
        );
        let publication = bus.publish(committed.clone(), ProducerChanged).await?;
        let history = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let history = runtime.store().load(&demo_stream()).await?;
                if history.len() == 3 {
                    return Ok::<_, rostfrei::EventStoreError>(history);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        let rented = history.last().ok_or("mapped command event")?;
        assert_eq!(rented.event_type(), "bicycle-rented");
        assert_eq!(rented.correlation_id(), source.correlation_id());
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
        // Domain commit precedes response publication; wait for actual response
        // evidence rather than treating readable history as its acknowledgement.
        let sequence = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let sequence = responses.info().await?.state.last_sequence;
                if sequence > 0 {
                    return Ok::<_, Box<dyn Error + Send + Sync>>(sequence);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        let response = responses.get_raw_message(sequence).await?;
        let response: rostfrei_messaging_core::CommandResponse =
            serde_json::from_slice(&response.payload)?;
        assert_eq!(
            rented.causation_id().map(rostfrei::CausationId::as_str),
            Some(response.command_message_id().as_str())
        );
        assert!(matches!(
            response.outcome(),
            rostfrei_messaging_core::CommandResponseOutcome::Accepted
        ));
        let duplicate = bus.publish(committed, ProducerChanged).await?;
        assert_eq!(duplicate.message_id(), publication.message_id());
        assert!(duplicate.duplicate());
        if runtime.config().context.traffic_scope() == rostfrei::TrafficScope::Test {
            assert_eq!(
                normal.store().load(&demo_stream()).await?.len(),
                2,
                "Test mapper touched Normal history"
            );
        }
    }
    assert_eq!(host.runtime().store().load(&demo_stream()).await?.len(), 3);
    let history = host.runtime().store().load(&demo_stream()).await?;
    let source = history.first().ok_or("source")?;
    let other = application
        .test_bounded_context(OtherProducerChanged::BOUNDED_CONTEXT.ok_or("producer")?)?;
    IntegrationEventBus::new(
        other.clone(),
        Arc::new(
            connection.messaging_adapter(host.runtime().config().messaging.topology().clone()),
        ),
    )
    .publish(
        CommittedEventContext::new(source)?,
        OtherProducerChanged {
            note: "other codec".into(),
        },
    )
    .await?;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), other_received.recv()).await?,
        Some(other.integration_event_address(OtherProducerChanged::EVENT_NAME)?)
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = feed.snapshot();
            if let Some(summary) = snapshot.items.iter().find(|flow| flow.message_count == 4) {
                let flow = feed.flow(&summary.id)?;
                assert!(
                    flow.message_series
                        .messages()
                        .iter()
                        .any(|message| message.payload()
                            == Some(&serde_json::json!({"note":"other codec"})))
                );
                assert_eq!(
                    snapshot.discarded_messages, "0",
                    "producer-specific codecs must not collide"
                );
                return Ok::<_, Box<dyn Error + Send + Sync>>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    host.shutdown().await?;
    connection.drain().await?;
    Ok(())
}
