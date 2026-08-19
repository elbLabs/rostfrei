#![allow(clippy::panic_in_result_fn)]

use std::{error::Error, sync::Arc, time::Duration};

use async_nats::jetstream::{Message, consumer::PullConsumer};
use bike_rental::{
    BikeRentalCommand, BikeRentalNatsConfig, BikeRentalNatsError, BikeRentalNatsResourceLimits,
    BikeRentalNatsRuntime, rental_fleet::RentBicycle,
};
use futures_util::StreamExt as _;
use rostfrei::{Command, CommandBus, DynamicCommandRequest, OperationId, RoutedCommand};
use rostfrei_messaging_core::{CommandAddress, CommandEnvelope, ConsumerConfig};
use rostfrei_nats::{
    NatsConnection, NatsConnectionConfig, NatsError, ServerVersion, connect,
    provision_application_messaging, provision_durable_consumer, provision_event_store,
};
use rostfrei_testing::integration::nats_url;
use serde_json::json;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct Fixture {
    connection: NatsConnection,
    config: BikeRentalNatsConfig,
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        let application = format!("subscription-{}", uuid::Uuid::now_v7().simple());
        let config = BikeRentalNatsConfig::new_with_resource_limits(
            &application,
            BikeRentalNatsResourceLimits::from_env()?,
        )?;
        let connection = connect(
            &NatsConnectionConfig::new(&application, nats_url()?)
                .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
        )
        .await?;
        provision_application_messaging(connection.jetstream(), config.messaging()).await?;
        provision_event_store(connection.jetstream(), config.event_store()).await?;
        Ok(Self { connection, config })
    }

    // Snapshot the old example's schema-derived identity without sharing the new
    // subscription-generation constant, so a naming regression fails reprovisioning.
    fn legacy_consumer(&self, schema_version: u32) -> TestResult<ConsumerConfig<CommandAddress>> {
        let context = self.config.context();
        let current = self
            .config
            .command_route(BikeRentalCommand::RentBicycle)
            .consumer();
        Ok(ConsumerConfig::new(
            context.consumer_name(RentBicycle::LOCAL_ID, schema_version)?,
            context.durable_name(RentBicycle::LOCAL_ID, schema_version)?,
            context.command_address(RentBicycle::LOCAL_ID)?,
            current.ack_wait(),
            current.processing_timeout(),
            current.concurrency(),
            current.maximum_delivery_attempts(),
        )?)
    }

    async fn publish(&self, operation: &str, schema_version: u32) -> TestResult<(u64, Vec<u8>)> {
        let bus = CommandBus::new(
            self.config.context().clone(),
            Arc::new(
                self.connection
                    .messaging_adapter(self.config.messaging().topology().clone()),
            ),
        );
        let message = bus.encode_dynamic(DynamicCommandRequest::new(
            OperationId::new(operation)?,
            RentBicycle::LOCAL_ID,
            schema_version,
            json!({ "fleet_id": "city-fleet", "bicycle_id": "bike-42" }),
        )?)?;
        let payload = message.payload().to_vec();
        let ack = self
            .connection
            .publisher(self.config.messaging().topology().clone())
            .publish_command_with_ack(message.message().clone(), Duration::from_secs(5))
            .await?;
        Ok((ack.sequence(), payload))
    }

    async fn reprovision(&self) -> Result<BikeRentalNatsRuntime, BikeRentalNatsError> {
        BikeRentalNatsRuntime::provision_with_resource_limits(
            self.connection.clone(),
            self.config.application().as_str(),
            self.config.resource_limits(),
        )
        .await
    }

    async fn cleanup(&self) -> TestResult {
        let topology = self.config.messaging().topology();
        for stream in [
            topology.command_stream().as_str(),
            topology.command_response_stream().as_str(),
            topology.integration_event_stream().as_str(),
            topology.quarantine_stream().as_str(),
            self.config.event_store().stream_name(),
        ] {
            self.connection.jetstream().delete_stream(stream).await?;
        }
        self.connection.drain().await?;
        Ok(())
    }
}

async fn next_message(consumer: &PullConsumer) -> TestResult<Message> {
    let mut messages = consumer
        .fetch()
        .max_messages(1)
        .expires(Duration::from_secs(5))
        .messages()
        .await?;
    messages.next().await.ok_or("expected a pending command")?
}

#[tokio::test]
async fn reprovisioning_preserves_existing_durable_progress_and_pending_older_schema() -> TestResult
{
    let fixture = Fixture::new().await?;
    let result: TestResult = Box::pin(async {
        let legacy = fixture.legacy_consumer(2)?;
        provision_durable_consumer(
            fixture.connection.jetstream(),
            fixture.config.messaging().topology(),
            &legacy,
        )
        .await?;
        let mut stream = fixture
            .connection
            .jetstream()
            .get_stream(
                fixture
                    .config
                    .messaging()
                    .topology()
                    .command_stream()
                    .as_str(),
            )
            .await?;
        let mut consumer: PullConsumer =
            stream.get_consumer(legacy.durable_name().as_str()).await?;
        let (acknowledged_sequence, _) = fixture.publish("acknowledged", 2).await?;
        let (pending_sequence, pending_payload) = fixture.publish("pending", 1).await?;
        let acknowledged = next_message(&consumer).await?;
        assert_eq!(acknowledged.info()?.stream_sequence, acknowledged_sequence);
        acknowledged.double_ack().await?;
        let before = consumer.info().await?.clone();
        assert_eq!(before.ack_floor.stream_sequence, acknowledged_sequence);
        assert_eq!(before.num_pending, 1);
        assert_eq!(before.num_ack_pending, 0);

        // Reuse the released --v2 identity even with an older payload still queued.
        // Naming independence across typed schema definitions is covered by the unit test.
        assert_eq!(RentBicycle::SCHEMA_VERSION, 2);
        let runtime = fixture.reprovision().await?;
        let route = runtime
            .config()
            .command_route(BikeRentalCommand::RentBicycle);
        assert_eq!(legacy.name(), route.consumer().name());
        assert_eq!(legacy.durable_name(), route.consumer().durable_name());
        assert_eq!(legacy.address(), route.address());
        let mut reused: PullConsumer = stream
            .get_consumer(route.consumer().durable_name().as_str())
            .await?;
        let after = reused.info().await?;
        assert_eq!(after.created, before.created);
        assert_eq!(after.ack_floor, before.ack_floor);
        assert_eq!(after.delivered, before.delivered);
        assert_eq!(after.num_pending, before.num_pending);
        assert_eq!(after.num_ack_pending, before.num_ack_pending);
        let state = &stream.info().await?.state;
        assert_eq!(
            state.consumer_count,
            runtime.config().command_routes().len()
        );
        assert_eq!(state.messages, 1);
        let retained = stream.get_raw_message(pending_sequence).await?;
        assert_eq!(retained.payload.as_ref(), pending_payload);

        let pending = next_message(&reused).await?;
        assert_eq!(pending.info()?.stream_sequence, pending_sequence);
        assert_eq!(pending.payload.as_ref(), pending_payload);
        let envelope: CommandEnvelope<RoutedCommand> = serde_json::from_slice(&pending.payload)?;
        assert_eq!(envelope.payload().schema_version(), 1);
        pending.double_ack().await?;
        Ok(())
    })
    .await;
    let cleanup = fixture.cleanup().await;
    result?;
    cleanup
}

#[tokio::test]
async fn startup_preserves_conflicting_legacy_durable_and_queued_payload() -> TestResult {
    let fixture = Fixture::new().await?;
    let result: TestResult = async {
        let legacy = fixture.legacy_consumer(1)?;
        let before = provision_durable_consumer(
            fixture.connection.jetstream(),
            fixture.config.messaging().topology(),
            &legacy,
        )
        .await?;
        // WorkQueue filters conflict even when the old durable has no queued work.
        assert!(matches!(
            fixture.reprovision().await,
            Err(BikeRentalNatsError::Nats(NatsError::Provisioning))
        ));
        let (sequence, payload) = fixture.publish("legacy-pending", 1).await?;
        assert!(matches!(
            fixture.reprovision().await,
            Err(BikeRentalNatsError::Nats(NatsError::Provisioning))
        ));

        let mut stream = fixture
            .connection
            .jetstream()
            .get_stream(
                fixture
                    .config
                    .messaging()
                    .topology()
                    .command_stream()
                    .as_str(),
            )
            .await?;
        let consumer: PullConsumer = stream.get_consumer(legacy.durable_name().as_str()).await?;
        let after = consumer.cached_info();
        assert_eq!(after.created, before.created);
        assert_eq!(after.ack_floor, before.ack_floor);
        assert_eq!(after.num_pending, 1);
        let state = &stream.info().await?.state;
        assert_eq!(state.consumer_count, 1);
        assert_eq!(state.messages, 1);
        assert_eq!(
            stream.get_raw_message(sequence).await?.payload.as_ref(),
            payload
        );
        Ok(())
    }
    .await;
    let cleanup = fixture.cleanup().await;
    result?;
    cleanup
}
