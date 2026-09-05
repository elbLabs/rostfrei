use super::{
    ApplicationBindings, Arc, CommandAddress, CommandBus, CommandBusTransport,
    CommandMessageAdapter, Duration, MessageHandler, NatsConnection, NatsEventStore, NatsHostError,
    NatsMessagingAdapter, NatsRuntimeConfig, async_trait,
};
use rostfrei::{CommandProcessor, CommandProcessorBuilder, TrustedCommandContext};
use rostfrei_core::{EventHistory, EventStore, StreamDirectory};
use rostfrei_nats::TrustedCommandContextResolver;

type ConfigureProcessor =
    Arc<dyn Fn(CommandProcessorBuilder) -> CommandProcessorBuilder + Send + Sync>;

/// The effective execution policy, retained when a Test authority is recreated.
/// Trusted compatibility is durable, but has no per-call configured policy admission.
#[derive(Clone, Default)]
pub enum NatsExecutionProfile {
    #[default]
    TrustedCompatibility,
    Configured {
        configure: ConfigureProcessor,
        resolver: Arc<dyn TrustedCommandContextResolver>,
        sender: TrustedCommandContext,
    },
}

impl NatsExecutionProfile {
    /// Configure the ordinary processor builder (authorizer, audit, retry and
    /// rejection codec). The host supplies the current backend and its one event
    /// codec registry. This closure is called again after a successful reconnect.
    /// Caller identity is supplied explicitly, never inferred from actor or token.
    pub fn configured(
        configure: impl Fn(CommandProcessorBuilder) -> CommandProcessorBuilder + Send + Sync + 'static,
        resolver: Arc<dyn TrustedCommandContextResolver>,
        sender: TrustedCommandContext,
    ) -> Self {
        Self::Configured {
            configure: Arc::new(configure),
            resolver,
            sender,
        }
    }
}

pub(super) struct Backend {
    pub store: NatsEventStore,
    pub messaging: Arc<NatsMessagingAdapter>,
    pub handler: Arc<dyn MessageHandler<CommandAddress>>,
    pub transport: CommandBusTransport,
}

impl Backend {
    pub fn new(
        connection: &NatsConnection,
        config: &NatsRuntimeConfig,
        bindings: &ApplicationBindings,
        store: NatsEventStore,
    ) -> Result<Self, NatsHostError> {
        let messaging = connection
            .messaging_adapter(config.messaging.topology().clone())
            .with_response_timeout(Duration::from_secs(30));
        let (messaging, handler) = match &config.execution_profile {
            NatsExecutionProfile::TrustedCompatibility => {
                let processor = bindings
                    .command_processor(config.context.name().clone(), Arc::new(store.clone()))?;
                let handler = messaging.command_handler(Arc::new(processor));
                (messaging, handler)
            }
            NatsExecutionProfile::Configured {
                configure,
                resolver,
                sender,
            } => {
                let mut processor = configure(CommandProcessor::builder(
                    config.context.clone(),
                    Arc::new(store.clone()),
                ))
                .event_codecs(bindings.codecs.clone())
                .build()?;
                bindings.bind_processor(&mut processor)?;
                let service = Arc::new(processor);
                let handler =
                    messaging.command_handler_with_resolver(service.clone(), resolver.clone());
                let messaging = messaging
                    .with_outcome_service(service, sender.clone())
                    .with_replay_publication();
                (messaging, handler)
            }
        };
        let messaging = Arc::new(messaging);
        let adapter: Arc<dyn CommandMessageAdapter> = messaging.clone();
        Ok(Self {
            store,
            messaging,
            handler: Arc::new(handler),
            transport: CommandBusTransport::new(CommandBus::new(config.context.clone(), adapter)),
        })
    }
}

/// A short-lived snapshot of the current connected backend; no lock is held across
/// async work. Runtime submission/reset admission owns the encompassing lifecycle.
#[derive(Clone)]
pub(super) struct CurrentBackend(Arc<std::sync::RwLock<Arc<Backend>>>);
impl CurrentBackend {
    pub fn new(backend: Backend) -> Self {
        Self(Arc::new(std::sync::RwLock::new(Arc::new(backend))))
    }
    pub fn snapshot(&self) -> Arc<Backend> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn replace(&self, backend: Backend) {
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(backend);
    }
}

// Diagnostic/discovery and fixture capabilities follow the current Test handle.
// Reads do not assert authority health, and are not a substitute for admission.
#[async_trait]
impl EventHistory for CurrentBackend {
    async fn load(
        &self,
        stream: &rostfrei_core::StreamId,
    ) -> Result<Vec<rostfrei_core::RecordedEvent>, rostfrei_core::EventStoreError> {
        self.snapshot().store.load(stream).await
    }
}
#[async_trait]
impl StreamDirectory for CurrentBackend {
    async fn list_streams(
        &self,
        aggregate: &rostfrei_core::AggregateType,
    ) -> Result<Vec<rostfrei_core::StreamSummary>, rostfrei_core::EventStoreError> {
        self.snapshot().store.list_streams(aggregate).await
    }
}
#[async_trait]
impl EventStore for CurrentBackend {
    async fn load_completed_outcome_in_context(
        &self,
        context: &rostfrei::BoundedContextName,
        id: &rostfrei_core::OperationId,
    ) -> Result<Option<rostfrei_core::InlineCompletedOutcome>, rostfrei_core::EventStoreError> {
        self.snapshot()
            .store
            .load_completed_outcome_in_context(context, id)
            .await
    }
    async fn commit_outcome(
        &self,
        transaction: rostfrei_core::EventTransaction,
    ) -> Result<rostfrei_core::CompletedAppendOutcome, rostfrei_core::EventStoreError> {
        self.snapshot().store.commit_outcome(transaction).await
    }
    async fn append(
        &self,
        stream: &rostfrei_core::StreamId,
        expected: rostfrei_core::ExpectedVersion,
        batch: rostfrei_core::EventBatch,
    ) -> Result<rostfrei_core::AppendOutcome, rostfrei_core::EventStoreError> {
        self.snapshot().store.append(stream, expected, batch).await
    }
    async fn load_transaction_receipt(
        &self,
        id: &rostfrei_core::OperationId,
    ) -> Result<Option<rostfrei_core::TransactionReceipt>, rostfrei_core::EventStoreError> {
        self.snapshot().store.load_transaction_receipt(id).await
    }
    async fn load_transaction_receipt_in_context(
        &self,
        context: &rostfrei::BoundedContextName,
        id: &rostfrei_core::OperationId,
    ) -> Result<Option<rostfrei_core::TransactionReceipt>, rostfrei_core::EventStoreError> {
        self.snapshot()
            .store
            .load_transaction_receipt_in_context(context, id)
            .await
    }
    async fn append_transaction(
        &self,
        transaction: rostfrei_core::EventTransaction,
    ) -> Result<rostfrei_core::TransactionAppendOutcome, rostfrei_core::EventStoreError> {
        self.snapshot().store.append_transaction(transaction).await
    }
}
