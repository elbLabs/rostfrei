#![allow(clippy::panic_in_result_fn)]

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
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
use rostfrei::{
    Aggregate, AppendOutcome, CommandDefinition, EventBatch, EventHistory, EventStore,
    ExpectedVersion, InMemoryEventStore, RecordedEvent, StreamAggregateId, StreamId,
};
use rostfrei_control_plane::{
    DispatchAdapter, DispatchInvocation, DispatchObserver, DispatchOutcome, DispatchPublication,
    dispatch_fingerprint,
};
use rostfrei_messaging_core::{
    CallerMetadata, CommandAddress, CommandEnvelope, CommandPublisher, CommandResponse,
    CommandResponseAddress, CommandResponsePublisher, CommandResponseReadError,
    CommandResponseReadErrorKind, CommandResponseReader, CorrelationId, DeliveryDisposition,
    DeliveryInfo, MessageDelivery, MessageHandler, MessageId, OperationId, OutboundMessage,
    PublishError, PublishErrorKind, PublishReceipt, derive_command_response_address,
};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify, Semaphore};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Default)]
struct FakeBroker {
    commands: Arc<Mutex<Vec<OutboundMessage<CommandAddress>>>>,
    command_ids: Arc<Mutex<HashSet<String>>>,
    command_changed: Arc<Notify>,
    response_messages: Arc<Mutex<Vec<OutboundMessage<CommandResponseAddress>>>>,
    responses: Arc<Mutex<HashMap<String, CommandResponse>>>,
    response_changed: Arc<Notify>,
}

impl FakeBroker {
    async fn command(&self, index: usize) -> OutboundMessage<CommandAddress> {
        loop {
            let changed = self.command_changed.notified();
            let message = self.commands.lock().await.get(index).cloned();
            if let Some(message) = message {
                return message;
            }
            changed.await;
        }
    }
}

#[async_trait]
impl CommandPublisher for FakeBroker {
    async fn publish_command(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        let duplicate = !self
            .command_ids
            .lock()
            .await
            .insert(message.message_id().as_str().to_owned());
        self.commands.lock().await.push(message);
        self.command_changed.notify_waiters();
        Ok(PublishReceipt::new(duplicate))
    }
}

#[async_trait]
impl CommandResponsePublisher for FakeBroker {
    async fn publish_command_response(
        &self,
        message: OutboundMessage<CommandResponseAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        let response: CommandResponse = serde_json::from_slice(message.payload())
            .map_err(|_| PublishError::new(PublishErrorKind::Rejected))?;
        if response.message_id() != message.message_id() {
            return Err(PublishError::new(PublishErrorKind::Rejected));
        }
        let expected_address = derive_command_response_address(
            response.command_address(),
            response.operation_id(),
            response.command_message_id(),
        )
        .map_err(|_| PublishError::new(PublishErrorKind::Rejected))?;
        if &expected_address != message.address() {
            return Err(PublishError::new(PublishErrorKind::Rejected));
        }
        self.response_messages.lock().await.push(message.clone());
        let mut responses = self.responses.lock().await;
        if let Some(existing) = responses.get(message.address().as_str()) {
            return if existing == &response {
                Ok(PublishReceipt::new(true))
            } else {
                Err(PublishError::new(PublishErrorKind::Rejected))
            };
        }
        responses.insert(message.address().as_str().to_owned(), response);
        drop(responses);
        self.response_changed.notify_waiters();
        Ok(PublishReceipt::new(false))
    }
}

#[async_trait]
impl CommandResponseReader for FakeBroker {
    async fn read_command_response(
        &self,
        address: &CommandResponseAddress,
        expected_operation_id: &OperationId,
        expected_command_message_id: &MessageId,
        read_timeout: Duration,
    ) -> Result<CommandResponse, CommandResponseReadError> {
        tokio::time::timeout(read_timeout, async {
            loop {
                let changed = self.response_changed.notified();
                let response = self.responses.lock().await.get(address.as_str()).cloned();
                if let Some(response) = response {
                    if response.operation_id() != expected_operation_id
                        || response.command_message_id() != expected_command_message_id
                        || !derive_command_response_address(
                            response.command_address(),
                            response.operation_id(),
                            response.command_message_id(),
                        )
                        .is_ok_and(|derived| &derived == address)
                    {
                        return Err(CommandResponseReadError::new(
                            CommandResponseReadErrorKind::IdentityConflict,
                        ));
                    }
                    return Ok(response);
                }
                changed.await;
            }
        })
        .await
        .map_err(|_| CommandResponseReadError::new(CommandResponseReadErrorKind::Timeout))?
    }
}

#[derive(Clone, Default)]
struct RecordingObserver {
    publications: Arc<Mutex<Vec<DispatchPublication>>>,
    changed: Arc<Notify>,
}

impl RecordingObserver {
    async fn publication(&self, index: usize) -> DispatchPublication {
        loop {
            let changed = self.changed.notified();
            let publication = self.publications.lock().await.get(index).cloned();
            if let Some(publication) = publication {
                return publication;
            }
            changed.await;
        }
    }
}

#[async_trait]
impl DispatchObserver for RecordingObserver {
    async fn command_published(&self, publication: DispatchPublication) {
        self.publications.lock().await.push(publication);
        self.changed.notify_waiters();
    }
}

#[derive(Clone, Default)]
struct RecordingPublisher {
    messages: Arc<Mutex<Vec<OutboundMessage<CommandAddress>>>>,
    seen: Arc<Mutex<HashSet<String>>>,
}

#[async_trait]
impl CommandPublisher for RecordingPublisher {
    async fn publish_command(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        let duplicate = !self
            .seen
            .lock()
            .await
            .insert(message.message_id().as_str().to_owned());
        self.messages.lock().await.push(message);
        Ok(PublishReceipt::new(duplicate))
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

struct ImmediateAcceptedReader {
    command_address: CommandAddress,
}

#[async_trait]
impl CommandResponseReader for ImmediateAcceptedReader {
    async fn read_command_response(
        &self,
        _address: &CommandResponseAddress,
        expected_operation_id: &OperationId,
        expected_command_message_id: &MessageId,
        _read_timeout: Duration,
    ) -> Result<CommandResponse, CommandResponseReadError> {
        CommandResponse::accepted(
            MessageId::new(format!("response-{}", expected_command_message_id.as_str())).unwrap(),
            expected_command_message_id.clone(),
            self.command_address.clone(),
            expected_operation_id.clone(),
            CorrelationId::new(expected_operation_id.as_str()).unwrap(),
        )
        .map_err(|_| CommandResponseReadError::new(CommandResponseReadErrorKind::InvalidResponse))
    }
}

struct TimeoutOnceAcceptedReader {
    command_address: CommandAddress,
    attempts: Arc<AtomicUsize>,
}

#[async_trait]
impl CommandResponseReader for TimeoutOnceAcceptedReader {
    async fn read_command_response(
        &self,
        _address: &CommandResponseAddress,
        expected_operation_id: &OperationId,
        expected_command_message_id: &MessageId,
        _read_timeout: Duration,
    ) -> Result<CommandResponse, CommandResponseReadError> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(CommandResponseReadError::new(
                CommandResponseReadErrorKind::Timeout,
            ));
        }
        CommandResponse::accepted(
            MessageId::new(format!("response-{}", expected_command_message_id.as_str())).unwrap(),
            expected_command_message_id.clone(),
            self.command_address.clone(),
            expected_operation_id.clone(),
            CorrelationId::new(expected_operation_id.as_str()).unwrap(),
        )
        .map_err(|_| CommandResponseReadError::new(CommandResponseReadErrorKind::InvalidResponse))
    }
}

struct UnavailableResponseReader;

#[async_trait]
impl CommandResponseReader for UnavailableResponseReader {
    async fn read_command_response(
        &self,
        _address: &CommandResponseAddress,
        _expected_operation_id: &OperationId,
        _expected_command_message_id: &MessageId,
        _read_timeout: Duration,
    ) -> Result<CommandResponse, CommandResponseReadError> {
        Err(CommandResponseReadError::new(
            CommandResponseReadErrorKind::Unavailable,
        ))
    }
}

#[derive(Clone)]
struct CountingStore {
    inner: InMemoryEventStore,
    loads: Arc<AtomicUsize>,
}

impl CountingStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            loads: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn load_count(&self) -> usize {
        self.loads.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl EventHistory for CountingStore {
    async fn load(
        &self,
        stream_id: &StreamId,
    ) -> Result<Vec<RecordedEvent>, rostfrei::EventStoreError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.inner.load(stream_id).await
    }
}

#[async_trait]
impl EventStore for CountingStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, rostfrei::EventStoreError> {
        self.inner.append(stream_id, expected_version, batch).await
    }
}

#[derive(Clone)]
struct BlockingResponsePublisher {
    messages: Arc<Mutex<Vec<OutboundMessage<CommandResponseAddress>>>>,
    entered: Arc<Notify>,
    release: Arc<Semaphore>,
}

impl Default for BlockingResponsePublisher {
    fn default() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Semaphore::new(0)),
        }
    }
}

#[async_trait]
impl CommandResponsePublisher for BlockingResponsePublisher {
    async fn publish_command_response(
        &self,
        message: OutboundMessage<CommandResponseAddress>,
    ) -> Result<PublishReceipt, PublishError> {
        self.messages.lock().await.push(message);
        self.entered.notify_one();
        self.release
            .acquire()
            .await
            .expect("test response publication gate remains open")
            .forget();
        Ok(PublishReceipt::new(false))
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
    attempt: u32,
) -> TestResult<MessageDelivery<CommandAddress>> {
    Ok(MessageDelivery::new(
        message.address().clone(),
        message.message_id().clone(),
        message.payload().to_vec(),
        CallerMetadata::new(),
        DeliveryInfo::new(attempt, 0, u64::from(attempt), u64::from(attempt))?,
    )?)
}

fn adapter(config: &BikeRentalNatsConfig, broker: &FakeBroker) -> Arc<NatsCommandDispatchAdapter> {
    let publisher: Arc<dyn CommandPublisher> = Arc::new(broker.clone());
    let reader: Arc<dyn CommandResponseReader> = Arc::new(broker.clone());
    Arc::new(NatsCommandDispatchAdapter::new(
        publisher,
        reader,
        config.command_address().clone(),
    ))
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
        config
            .messaging()
            .topology()
            .command_response_stream()
            .as_str(),
        "BIKE_RENTAL_DEMO_COMMAND_RESPONSES"
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
    let adapter = NatsCommandDispatchAdapter::new(
        Arc::new(publisher),
        Arc::new(ImmediateAcceptedReader {
            command_address: config.command_address().clone(),
        }),
        config.command_address().clone(),
    );
    let observer: Arc<dyn DispatchObserver> = Arc::new(RecordingObserver::default());
    let payload = json!({ "bicycle_id": "bike-42" });

    for invocation in [
        invocation("same-operation", payload.clone())?,
        invocation("same-operation", payload)?,
        invocation("same-operation", json!({ "bicycle_id": "bike-99" }))?,
        invocation("different-operation", json!({ "bicycle_id": "bike-42" }))?,
    ] {
        adapter.dispatch(invocation, Arc::clone(&observer)).await?;
    }

    let messages = messages.lock().await;
    let ids = messages
        .iter()
        .map(|message| message.message_id().clone())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 4);
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
    assert_ne!(ids[0], ids[3]);
    Ok(())
}

#[tokio::test]
async fn transient_publication_retries_the_exact_message_before_waiting_for_response() -> TestResult
{
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let publisher = FlakyPublisher::default();
    let attempts = Arc::clone(&publisher.attempts);
    let message_ids = Arc::clone(&publisher.message_ids);
    let adapter = NatsCommandDispatchAdapter::new(
        Arc::new(publisher),
        Arc::new(ImmediateAcceptedReader {
            command_address: config.command_address().clone(),
        }),
        config.command_address().clone(),
    );
    let observer = Arc::new(RecordingObserver::default());

    let receipt = adapter
        .dispatch(
            invocation("retry-operation", json!({ "bicycle_id": "bike-42" }))?,
            observer.clone(),
        )
        .await?;

    assert!(receipt.duplicate());
    assert!(matches!(receipt.outcome(), DispatchOutcome::Accepted));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    let message_ids = message_ids.lock().await.clone();
    assert_eq!(message_ids.len(), 2);
    assert_eq!(message_ids.first(), message_ids.get(1));
    assert!(observer.publication(0).await.duplicate());
    Ok(())
}

#[tokio::test]
async fn response_read_timeout_keeps_listening_without_republishing_command() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let publisher = RecordingPublisher::default();
    let messages = Arc::clone(&publisher.messages);
    let attempts = Arc::new(AtomicUsize::new(0));
    let adapter = NatsCommandDispatchAdapter::new(
        Arc::new(publisher),
        Arc::new(TimeoutOnceAcceptedReader {
            command_address: config.command_address().clone(),
            attempts: Arc::clone(&attempts),
        }),
        config.command_address().clone(),
    );

    let receipt = adapter
        .dispatch(
            invocation("response-read-retry", json!({ "bicycle_id": "bike-42" }))?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;

    assert!(matches!(receipt.outcome(), DispatchOutcome::Accepted));
    assert_eq!(messages.lock().await.len(), 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn unavailable_response_store_retries_delivery_without_executing() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let publisher = RecordingPublisher::default();
    let messages = Arc::clone(&publisher.messages);
    let adapter = NatsCommandDispatchAdapter::new(
        Arc::new(publisher),
        Arc::new(ImmediateAcceptedReader {
            command_address: config.command_address().clone(),
        }),
        config.command_address().clone(),
    );
    adapter
        .dispatch(
            invocation(
                "response-store-unavailable",
                json!({ "bicycle_id": "bike-42" }),
            )?,
            Arc::new(RecordingObserver::default()),
        )
        .await?;
    let command = messages
        .lock()
        .await
        .first()
        .cloned()
        .ok_or_else(|| io::Error::other("command was not published"))?;

    let store = CountingStore::new();
    seed_demo(&store).await?;
    let loads_before_delivery = store.load_count();
    let handler = RentBicycleMessageHandler::new(
        store.clone(),
        Arc::new(FakeBroker::default()),
        Arc::new(UnavailableResponseReader),
    );

    assert!(matches!(
        handler.handle(delivery(&command, 1)?).await,
        DeliveryDisposition::RetryAfter(_)
    ));
    assert_eq!(store.load_count(), loads_before_delivery);
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn dispatch_waits_for_durable_accepted_and_rejected_responses() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let broker = FakeBroker::default();
    let adapter = adapter(&config, &broker);
    let store = CountingStore::new();
    seed_demo(&store).await?;
    let response_publisher: Arc<dyn CommandResponsePublisher> = Arc::new(broker.clone());
    let response_reader: Arc<dyn CommandResponseReader> = Arc::new(broker.clone());
    let handler =
        RentBicycleMessageHandler::new(store.clone(), response_publisher, response_reader);

    let observer = Arc::new(RecordingObserver::default());
    let first_adapter = Arc::clone(&adapter);
    let first_observer = Arc::clone(&observer);
    let first_dispatch = tokio::spawn(async move {
        first_adapter
            .dispatch(
                invocation("rent-bike-42-first", json!({ "bicycle_id": "bike-42" })).unwrap(),
                first_observer,
            )
            .await
    });
    let publication = observer.publication(0).await;
    let first = broker.command(0).await;
    assert_eq!(
        publication.command_message_id(),
        first.message_id().as_str()
    );
    assert!(!first_dispatch.is_finished());

    assert_eq!(
        handler.handle(delivery(&first, 1)?).await,
        DeliveryDisposition::Acknowledge
    );
    let first_receipt = first_dispatch.await??;
    assert!(matches!(first_receipt.outcome(), DispatchOutcome::Accepted));
    assert_eq!(
        first_receipt.command_message_id(),
        first.message_id().as_str()
    );

    let first_envelope: CommandEnvelope<DispatchedCommand> =
        serde_json::from_slice(first.payload())?;
    let first_response_address = derive_command_response_address(
        first.address(),
        first_envelope.operation_id(),
        first.message_id(),
    )?;
    let first_response = broker
        .responses
        .lock()
        .await
        .get(first_response_address.as_str())
        .cloned()
        .ok_or_else(|| io::Error::other("accepted response was not retained"))?;
    assert_eq!(
        first_response.message_id().as_str(),
        first_receipt.response_message_id()
    );
    assert_eq!(
        first_response.correlation_id(),
        first_envelope.correlation_id()
    );

    let loads_after_first_delivery = store.load_count();
    assert_eq!(
        handler.handle(delivery(&first, 2)?).await,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(store.load_count(), loads_after_first_delivery);
    let response_attempts = broker.response_messages.lock().await;
    assert_eq!(response_attempts.len(), 1);
    drop(response_attempts);
    let history = store.load(&demo_stream()).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(
        history[1]
            .causation_id()
            .map(rostfrei_messaging_core::CausationId::as_str),
        Some(first.message_id().as_str())
    );
    assert_eq!(
        history[1]
            .correlation_id()
            .map(rostfrei_messaging_core::CorrelationId::as_str),
        Some(first_envelope.correlation_id().as_str())
    );

    let second_observer = Arc::new(RecordingObserver::default());
    let second_adapter = Arc::clone(&adapter);
    let task_observer = Arc::clone(&second_observer);
    let second_dispatch = tokio::spawn(async move {
        second_adapter
            .dispatch(
                invocation("rent-bike-42-second", json!({ "bicycle_id": "bike-42" })).unwrap(),
                task_observer,
            )
            .await
    });
    second_observer.publication(0).await;
    let second = broker.command(1).await;
    assert!(!second_dispatch.is_finished());
    assert_eq!(
        handler.handle(delivery(&second, 1)?).await,
        DeliveryDisposition::Acknowledge
    );
    let second_receipt = second_dispatch.await??;
    let DispatchOutcome::Rejected(rejection) = second_receipt.outcome() else {
        return Err(io::Error::other("second rental was not rejected").into());
    };
    assert_eq!(rejection.classification, "conflict");
    assert_eq!(rejection.code, "BICYCLE_UNAVAILABLE");
    assert_eq!(rejection.details.as_ref().unwrap()["bicycle_id"], "bike-42");
    assert_eq!(store.load(&demo_stream()).await?.len(), 2);
    assert_eq!(broker.responses.lock().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn command_handler_does_not_ack_until_response_publication_completes() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let broker = FakeBroker::default();
    let dispatch_adapter = adapter(&config, &broker);
    let observer = Arc::new(RecordingObserver::default());
    let command_task = tokio::spawn(async move {
        dispatch_adapter
            .dispatch(
                invocation("response-before-ack", json!({ "bicycle_id": "bike-42" })).unwrap(),
                observer,
            )
            .await
    });
    let command = broker.command(0).await;

    let store = InMemoryEventStore::new();
    seed_demo(&store).await?;
    let publisher = BlockingResponsePublisher::default();
    let entered = Arc::clone(&publisher.entered);
    let release = Arc::clone(&publisher.release);
    let messages = Arc::clone(&publisher.messages);
    let handler = Arc::new(RentBicycleMessageHandler::new(
        store,
        Arc::new(publisher),
        Arc::new(broker.clone()),
    ));
    let handle_delivery = delivery(&command, 1)?;
    let handling = tokio::spawn(async move { handler.handle(handle_delivery).await });

    entered.notified().await;
    assert_eq!(messages.lock().await.len(), 1);
    assert!(!handling.is_finished());
    release.add_permits(1);
    assert_eq!(handling.await?, DeliveryDisposition::Acknowledge);
    command_task.abort();
    let _ = command_task.await;
    Ok(())
}
