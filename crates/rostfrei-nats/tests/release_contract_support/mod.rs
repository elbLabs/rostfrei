//! Shared fixtures for the command execution/replay contracts, not a replacement executor.
#![allow(dead_code)]

use async_trait::async_trait;
pub use rostfrei_core::OperationId;
pub use rostfrei_core::*;
pub use rostfrei_messaging_core::*;
pub use rostfrei_nats::*;
pub use rostfrei_runtime::{
    CommandBus, CommandProcessor, CommandRequest, InMemoryMessagingAdapter,
};
use serde::{Deserialize, Serialize};
pub use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub use std::time::Duration;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const WAIT: Duration = Duration::from_secs(8);
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[derive(rostfrei_runtime::BoundedContext)]
#[domain(id = "release-contract", label = "Release contract")]
pub struct ReleaseContract;

#[derive(Clone, Debug, rostfrei_runtime::Command)]
#[domain(context = ReleaseContract, id = "act", label = "Act")]
pub struct Act {
    pub target: String,
    pub action: String,
    pub item: String,
}

#[derive(Default, Debug)]
pub struct State {
    pub exists: bool,
    pub items: Vec<String>,
}
pub struct Target;
impl Aggregate for Target {
    type State = State;
    type Event = Changed;
    const BOUNDED_CONTEXT: &'static str = "release-contract";
    const AGGREGATE_TYPE: &'static str = "target";
    fn initial(_: &StreamId) -> State {
        State::default()
    }
    fn apply(state: &mut State, event: &Changed) {
        state.exists |= event.created;
        if let Some(item) = &event.item {
            state.items.push(item.clone());
        }
    }
}
#[derive(Deserialize, Serialize)]
pub struct Changed {
    pub created: bool,
    pub item: Option<String>,
}
impl Event for Changed {
    fn event_type(&self) -> &'static str {
        "changed"
    }
    fn schema_version(&self) -> u32 {
        1
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        serde_json::to_vec(self)
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::EncodingFailed, e.to_string()))
    }
    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        serde_json::from_slice(event.payload())
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::MalformedPayload, e.to_string()))
    }
}
pub struct Handler(pub Arc<AtomicUsize>);
#[async_trait]
impl CommandHandler<Act> for Handler {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if command.action == "noop" {
            return Ok(CommandDecision::Accepted);
        }
        let mut target = execution.load::<Target>(&command.target).await?;
        if command.action == "add" && !target.aggregate().state().exists {
            return Ok(CommandDecision::Rejected("target-not-found"));
        }
        target.aggregate_mut().raise(Changed {
            created: command.action == "create",
            item: (command.action == "add").then(|| command.item.clone()),
        });
        Ok(CommandDecision::Accepted)
    }
}
pub fn map_rejection(rejection: &&'static str) -> Result<CommandRejection, String> {
    CommandRejection::new(
        CommandRejectionClassification::NotFound,
        ApplicationErrorCode::new("contract.target-not-found").map_err(|e| e.to_string())?,
        *rejection,
        None,
    )
    .map_err(|e| e.to_string())
}
pub fn processor(
    context: &BoundedContext,
    store: Arc<dyn ExecutionStore>,
    calls: Arc<AtomicUsize>,
    codec: Option<BadCodec>,
) -> TestResult<Arc<CommandProcessor>> {
    let mut processor = CommandProcessor::new(context.name().clone(), store);
    processor.register::<Act, _>(Handler(calls), map_rejection)?;
    if let Some(codec) = codec {
        processor.register_codec::<Target, _>(codec)?;
    }
    Ok(Arc::new(processor))
}
pub fn request(operation: &str, target: &str, action: &str) -> TestResult<CommandRequest<Act>> {
    Ok(CommandRequest::new(
        OperationId::new(operation)?,
        Act {
            target: target.to_owned(),
            action: action.to_owned(),
            item: "I".to_owned(),
        },
    )
    .with_correlation_id(CorrelationId::new("contract-correlation")?)
    .with_causation_id(CausationId::new("contract-causation")?)
    .with_created_at(MessageTimestamp::from_unix_milliseconds(1_700_000_000_000)?))
}
pub fn stream(target: &str) -> TestResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new("target")?,
        AggregateId::new(target)?,
    ))
}
pub fn local_bus(context: &BoundedContext, processor: Arc<CommandProcessor>) -> CommandBus {
    CommandBus::new(
        context.clone(),
        Arc::new(InMemoryMessagingAdapter::new(processor)),
    )
}
pub fn rejected(outcome: &CommandResponseOutcome) -> bool {
    matches!(outcome, CommandResponseOutcome::Rejected(r) if r.code().as_str() == "contract.target-not-found")
}

/// Only aggregate loads are faulted. Real receipt lookup and real commits are retained.
pub struct FaultStore {
    pub inner: InMemoryEventStore,
    pub kind: EventStoreErrorKind,
    pub active: Arc<AtomicBool>,
}
#[async_trait]
impl OutcomeReader for FaultStore {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.inner.lookup_outcome(key).await
    }
}
#[async_trait]
impl ExecutionStore for FaultStore {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        Ok(Box::new(FaultSession {
            fault: self,
            inner: self.inner.begin_command().await?,
        }))
    }
}
struct FaultSession<'a> {
    fault: &'a FaultStore,
    inner: Box<dyn CommandCommitSession + 'a>,
}
#[async_trait]
impl EventHistory for FaultSession<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.fault.load(id).await
    }
}
#[async_trait]
impl CommandCommitSession for FaultSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        self.inner.commit_outcome(proposal).await
    }
}
#[async_trait]
impl EventHistory for FaultStore {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        if self.active.load(Ordering::SeqCst) && id.aggregate_id().as_str().starts_with("bad") {
            return Err(EventStoreError::new(
                self.kind,
                "controlled aggregate-load fault",
            ));
        }
        self.inner.load(id).await
    }
}
#[async_trait]
impl EventStore for FaultStore {
    async fn load_completed_outcome_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<rostfrei_core::InlineCompletedOutcome>, EventStoreError> {
        self.inner
            .load_completed_outcome_in_context(context, operation)
            .await
    }
    async fn commit_outcome(
        &self,
        transaction: EventTransaction,
    ) -> Result<rostfrei_core::CompletedAppendOutcome, EventStoreError> {
        self.inner.commit_outcome(transaction).await
    }
    async fn append(
        &self,
        id: &StreamId,
        version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.inner.append(id, version, batch).await
    }
    async fn load_transaction_receipt(
        &self,
        operation: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner.load_transaction_receipt(operation).await
    }
    async fn load_transaction_receipt_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner
            .load_transaction_receipt_in_context(context, operation)
            .await
    }
    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        self.inner.append_transaction(transaction).await
    }
}
#[derive(Clone, Copy, Debug)]
pub enum BadCodec {
    Decode,
    Encode,
}
impl EventCodec<Target> for BadCodec {
    fn encode(&self, event: &Changed, id: EventId) -> Result<NewEvent, EventCodecError> {
        if matches!(self, Self::Encode) {
            return Err(EventCodecError::new(
                EventCodecErrorKind::EncodingFailed,
                "deterministic permanent encode fault",
            ));
        }
        <JsonEventCodec as EventCodec<Target>>::encode(&JsonEventCodec, event, id)
    }
    fn decode(&self, event: &RecordedEvent) -> Result<Changed, EventCodecError> {
        if matches!(self, Self::Decode) {
            return Err(EventCodecError::new(
                EventCodecErrorKind::MalformedPayload,
                "deterministic permanent decode fault",
            ));
        }
        <JsonEventCodec as EventCodec<Target>>::decode(&JsonEventCodec, event)
    }
}
#[derive(Serialize, Deserialize)]
pub struct Trigger {
    pub target: String,
}
impl rostfrei_runtime::IntegrationEvent for Trigger {
    const EVENT_NAME: &'static str = "trigger";
    const SCHEMA_VERSION: u32 = 1;
}
pub struct Mapper;
impl rostfrei_runtime::IntegrationCommandMapper<Trigger> for Mapper {
    type Command = Act;
    type Error = &'static str;
    fn map(&self, event: &Trigger) -> Result<Act, Self::Error> {
        Ok(Act {
            target: event.target.clone(),
            action: "create".to_owned(),
            item: "I".to_owned(),
        })
    }
}
pub fn trigger(
    context: &BoundedContext,
    target: &str,
) -> TestResult<OutboundMessage<IntegrationEventAddress>> {
    let address = context.integration_event_address("trigger")?;
    let source = EventId::new(format!("source-{target}"))?;
    let version = SchemaVersion::new(1)?;
    let id = rostfrei_runtime::integration_message_id(&address, version, &source)?;
    let correlation = CorrelationId::new("trigger-correlation")?;
    let envelope = IntegrationEventEnvelope::new(
        EnvelopeContext::new(
            id.clone(),
            version,
            correlation.clone(),
            Some(CausationId::new(source.as_str())?),
        ),
        MessageTimestamp::from_unix_milliseconds(1_700_000_000_000)?,
        Trigger {
            target: target.to_owned(),
        },
    )?;
    Ok(OutboundMessage::json(address, id, &envelope)?.with_correlation_id(correlation))
}
pub fn command_delivery(
    encoded: &rostfrei_runtime::EncodedCommand,
) -> TestResult<MessageDelivery<CommandAddress>> {
    Ok(MessageDelivery::new_with_transport_context(
        encoded.address().clone(),
        encoded.message_id().clone(),
        encoded.payload().to_vec(),
        CallerMetadata::new(),
        Some(encoded.correlation_id().clone()),
        None,
        DeliveryInfo::new(1, 0, 1, 1)?,
    )?)
}

/// Aborts only the consumer task created by this fixture, including on assertion panic.
pub struct Worker(pub tokio::task::JoinHandle<Result<(), ConsumeError>>);
impl Drop for Worker {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct Fixture {
    pub context: BoundedContext,
    pub js: async_nats::jetstream::Context,
    pub messaging: ApplicationMessagingConfig,
    pub adapter: Arc<NatsMessagingAdapter>,
}
impl Fixture {
    pub async fn new(label: &str) -> TestResult<Self> {
        let application = ApplicationName::new(format!(
            "contract-{label}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))?;
        Self::for_application(application).await
    }
    pub async fn for_application(application: ApplicationName) -> TestResult<Self> {
        let context = application.bounded_context("release-contract")?;
        let messaging =
            ApplicationMessagingConfig::new(&application)?.with_max_bytes(64 * 1024 * 1024)?;
        // A missing broker is a setup failure, never a silent acceptance-test skip.
        let options = match (
            std::env::var("ROSTFREI_CONTRACT_NATS_USER"),
            std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD"),
        ) {
            (Ok(user), Ok(password)) => {
                async_nats::ConnectOptions::with_user_and_password(user, password)
            }
            (Err(_), Err(_)) => async_nats::ConnectOptions::new(),
            _ => return Err("both disposable broker credential variables must be set".into()),
        };
        let client = options.connect(std::env::var("ROSTFREI_NATS_URL")?).await?;
        let js = async_nats::jetstream::new(client);
        provision_application_messaging(&js, &messaging).await?;
        let adapter = Arc::new(
            NatsMessagingAdapter::new(
                NatsPublisher::new(js.clone(), messaging.topology().clone()),
                NatsCommandResponseReader::new(js.clone(), messaging.topology().clone()),
            )
            .with_response_timeout(Duration::from_secs(3)),
        );
        Ok(Self {
            context,
            js,
            messaging,
            adapter,
        })
    }
    pub async fn event_store(&self) -> TestResult<Arc<NatsEventStore>> {
        let config = NatsEventStoreConfig::for_bounded_context(&self.context)?
            .with_fresh_completed_outcome_epoch("private-contract-fixture")?
            .with_storage_limits(32 * 1024 * 1024, 524_288)?;
        provision_event_store(&self.js, &config).await?;
        Ok(Arc::new(
            NatsEventStore::connect(self.js.clone(), config).await?,
        ))
    }
    pub async fn command_worker(&self, processor: Arc<CommandProcessor>) -> TestResult<Worker> {
        let config = ConsumerConfig::new(
            self.context.consumer_name("act", 1)?,
            self.context.durable_name("act", 1)?,
            self.context.command_address("act")?,
            Duration::from_secs(5),
            Duration::from_secs(1),
            2,
            1000,
        )?;
        provision_durable_consumer(&self.js, self.messaging.topology(), &config).await?;
        let factory = NatsConsumerFactory::new(self.js.clone(), self.messaging.topology().clone());
        let consumer = <NatsConsumerFactory as MessageConsumerFactory<CommandAddress>>::create(
            &factory, config,
        )?;
        let handler = Arc::new(self.adapter.command_handler(processor));
        Ok(Worker(tokio::spawn(
            async move { consumer.run(handler).await },
        )))
    }
    pub async fn split_event_store(&self) -> TestResult<Arc<NatsEventStore>> {
        let config = NatsEventStoreConfig::for_bounded_context(&self.context)?
            .with_fresh_split_outcome_epoch("private-split-contract-fixture")?
            .with_storage_limits(32 * 1024 * 1024, 524_288)?;
        provision_event_store(&self.js, &config).await?;
        Ok(Arc::new(
            NatsEventStore::connect(self.js.clone(), config).await?,
        ))
    }
    pub async fn messages(&self, name: &str) -> TestResult<u64> {
        let mut stream = self.js.get_stream(name).await?;
        Ok(stream.info().await?.state.messages)
    }
    pub fn publisher(&self) -> NatsPublisher {
        NatsPublisher::new(self.js.clone(), self.messaging.topology().clone())
    }
    pub async fn quarantine(&self) -> TestResult<Vec<QuarantineRecord>> {
        let stream = self
            .js
            .get_stream(self.messaging.topology().quarantine_stream().as_str())
            .await?;
        let info = stream.cached_info();
        if info.state.messages == 0 {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for sequence in info.state.first_sequence..=info.state.last_sequence {
            let stored = stream.get_raw_message(sequence).await?;
            records.push(serde_json::from_slice(&stored.payload)?);
        }
        Ok(records)
    }
}
