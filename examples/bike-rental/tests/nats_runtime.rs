#![allow(clippy::panic_in_result_fn)]

use std::{
    error::Error,
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use bike_rental::{
    nats_runtime::{
        BikeRentalNatsConfig, DispatchedCommand, NatsCommandDispatchAdapter,
        RentBicycleMessageHandler,
    },
    rental::{RentBicycle, RentalFleetAggregate},
    runtime::{demo_stream, seed_demo},
};
use rostfrei::{Aggregate, CommandDefinition, InMemoryEventStore, StreamAggregateId};
use rostfrei_control_plane::{DispatchAdapter, DispatchInvocation, dispatch_fingerprint};
use rostfrei_messaging_core::{
    CallerMetadata, CommandAddress, CommandEnvelope, CommandPublisher, DeliveryDisposition,
    DeliveryInfo, MessageDelivery, MessageHandler, OutboundMessage, PublishError, PublishErrorKind,
    PublishReceipt,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Default)]
struct RecordingPublisher {
    messages: Arc<Mutex<Vec<OutboundMessage<CommandAddress>>>>,
}

#[async_trait]
impl CommandPublisher for RecordingPublisher {
    async fn publish_command(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        self.messages.lock().await.push(message);
        Ok(PublishReceipt::new(false))
    }
}

#[derive(Clone, Default)]
struct FlakyPublisher {
    attempts: Arc<AtomicUsize>,
    message_ids: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl CommandPublisher for FlakyPublisher {
    async fn publish_command(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        self.message_ids
            .lock()
            .await
            .push(message.message_id().as_str().to_owned());
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(PublishError::new(PublishErrorKind::Timeout))
        } else {
            Ok(PublishReceipt::new(true))
        }
    }
}

fn invocation(operation_id: &str, payload: Value) -> TestResult<DispatchInvocation> {
    let aggregate_type = RentalFleetAggregate::aggregate_type().into_owned();
    let fingerprint = dispatch_fingerprint(
        &aggregate_type,
        "city-fleet",
        RentBicycle::COMMAND_NAME,
        RentBicycle::SCHEMA_VERSION,
        &payload,
    );
    Ok(DispatchInvocation::new(
        rostfrei::OperationId::new(operation_id)?,
        fingerprint,
        aggregate_type,
        StreamAggregateId::new("city-fleet")?,
        RentBicycle::COMMAND_NAME,
        RentBicycle::SCHEMA_VERSION,
        payload,
    ))
}

fn delivery(
    message: &OutboundMessage<CommandAddress>,
    sequence: u64,
) -> TestResult<MessageDelivery<CommandAddress>> {
    Ok(MessageDelivery::new(
        message.address().clone(),
        message.message_id().clone(),
        message.payload().to_vec(),
        CallerMetadata::new(),
        DeliveryInfo::new(1, 0, sequence, sequence)?,
    )?)
}

#[test]
fn nats_configuration_uses_stable_application_scoped_resources() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;

    assert_eq!(
        config.command_address().as_str(),
        "bike-rental-demo.command.bike-rental.rent-bicycle"
    );
    assert_eq!(
        config.messaging().topology().command_stream().as_str(),
        "BIKE_RENTAL_DEMO_COMMANDS"
    );
    assert_eq!(
        config.event_store().stream_name(),
        "BIKE_RENTAL_DEMO__BIKE_RENTAL_DOMAIN_EVENTS"
    );
    assert_eq!(
        config.command_consumer().durable_name().as_str(),
        "bike-rental-demo--bike-rental--rent-bicycle--v1"
    );
    Ok(())
}

#[tokio::test]
async fn broker_deduplication_identity_includes_operation_and_content() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let publisher = RecordingPublisher::default();
    let messages = Arc::clone(&publisher.messages);
    let adapter =
        NatsCommandDispatchAdapter::new(Arc::new(publisher), config.command_address().clone());
    let payload = json!({ "bicycle_id": "bike-42" });

    adapter
        .dispatch(invocation("same-operation", payload.clone())?)
        .await?;
    adapter
        .dispatch(invocation("same-operation", payload)?)
        .await?;
    adapter
        .dispatch(invocation(
            "same-operation",
            json!({ "bicycle_id": "bike-99" }),
        )?)
        .await?;
    adapter
        .dispatch(invocation(
            "different-operation",
            json!({ "bicycle_id": "bike-42" }),
        )?)
        .await?;

    let messages = messages.lock().await;
    let first = messages
        .first()
        .ok_or_else(|| io::Error::other("first command was not published"))?
        .message_id()
        .clone();
    let exact_retry = messages
        .get(1)
        .ok_or_else(|| io::Error::other("exact retry was not published"))?
        .message_id()
        .clone();
    let changed_content = messages
        .get(2)
        .ok_or_else(|| io::Error::other("changed command was not published"))?
        .message_id()
        .clone();
    let changed_operation = messages
        .get(3)
        .ok_or_else(|| io::Error::other("new operation was not published"))?
        .message_id()
        .clone();
    drop(messages);
    assert_eq!(first, exact_retry);
    assert_ne!(first, changed_content);
    assert_ne!(first, changed_operation);
    Ok(())
}

#[tokio::test]
async fn transient_publication_retries_the_exact_message_before_completion() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let publisher = FlakyPublisher::default();
    let attempts = Arc::clone(&publisher.attempts);
    let message_ids = Arc::clone(&publisher.message_ids);
    let adapter =
        NatsCommandDispatchAdapter::new(Arc::new(publisher), config.command_address().clone());

    let receipt = adapter
        .dispatch(invocation(
            "retry-operation",
            json!({ "bicycle_id": "bike-42" }),
        )?)
        .await?;

    assert!(receipt.duplicate());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    let message_ids = message_ids.lock().await.clone();
    assert_eq!(message_ids.len(), 2);
    assert_eq!(message_ids.first(), message_ids.get(1));
    Ok(())
}

#[tokio::test]
async fn redelivery_is_an_exact_replay_and_a_new_rental_is_rejected() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let publisher = RecordingPublisher::default();
    let messages = Arc::clone(&publisher.messages);
    let adapter =
        NatsCommandDispatchAdapter::new(Arc::new(publisher), config.command_address().clone());
    let store = InMemoryEventStore::new();
    seed_demo(&store).await?;
    let handler = RentBicycleMessageHandler::new(store.clone());
    let payload = json!({ "bicycle_id": "bike-42" });

    adapter
        .dispatch(invocation("rent-bike-42-first", payload.clone())?)
        .await?;
    let first = messages
        .lock()
        .await
        .first()
        .cloned()
        .ok_or_else(|| io::Error::other("first command was not published"))?;
    let envelope: CommandEnvelope<DispatchedCommand> = serde_json::from_slice(first.payload())?;
    assert_eq!(envelope.message_id(), first.message_id());
    assert_eq!(envelope.payload().aggregate_id(), "city-fleet");
    assert_eq!(envelope.payload().command(), "rent-bicycle");
    assert_eq!(
        handler.handle(delivery(&first, 1)?).await,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(store.load(&demo_stream()).await?.len(), 2);
    assert_eq!(
        handler.handle(delivery(&first, 2)?).await,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(store.load(&demo_stream()).await?.len(), 2);

    adapter
        .dispatch(invocation("rent-bike-42-second", payload)?)
        .await?;
    let second = messages
        .lock()
        .await
        .get(1)
        .cloned()
        .ok_or_else(|| io::Error::other("second command was not published"))?;
    assert_eq!(
        handler.handle(delivery(&second, 3)?).await,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(store.load(&demo_stream()).await?.len(), 2);
    Ok(())
}
