//! Explicitly provisioned NATS runtime and optional, application-linked Tracer host.
//!
//! Test resources belong to the supplied application namespace. Until run identity
//! (#68) is available, use a dedicated broker or unique application namespace per
//! independent host. The reset gate coordinates this runtime's submissions only.

mod observation;

use crate::{
    ApplicationBindings, CommandBusTransport, CommandInvocation, CommandReceipt, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
    CorrelationObserver, OperationMode, TestRepository, TestScenarioReset, TestScenarioResetError,
    TracePayloadPolicy, Tracer,
    http::{self, HttpConfig},
};
use async_trait::async_trait;
use rostfrei::{
    CommandBus, CommandMessageAdapter, DomainEventDispatcher, EventStore, IntegrationEventBus,
    IntegrationMessageAdapter,
};
use rostfrei_fixtures::{Fixture, FixtureApplyReport, FixtureEventSet};
use rostfrei_messaging_core::{
    BoundedContext, CommandAddress, ConsumerConfig, IntegrationEventAddress,
    MessageConsumerFactory, MessageHandler, RetryDelay, TrafficScope,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnection, NatsCorrelationObserver, NatsDomainEventConsumer,
    NatsDomainEventConsumerConfig, NatsEventStore, NatsEventStoreConfig, NatsMessagingAdapter,
    provision_application_messaging, provision_domain_event_consumer, provision_durable_consumer,
    provision_event_store,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, RwLock, watch},
    task::JoinHandle,
};

#[derive(Debug, Error)]
pub enum NatsHostError {
    #[error(transparent)]
    Binding(#[from] crate::ApplicationBindingError),
    #[error(transparent)]
    Registration(#[from] crate::RuntimeRegistrationError),
    #[error(transparent)]
    Contract(#[from] rostfrei_messaging_core::ContractError),
    #[error(transparent)]
    Nats(#[from] rostfrei_nats::NatsError),
    #[error(transparent)]
    Store(#[from] rostfrei::EventStoreError),
    #[error(transparent)]
    Consumer(#[from] rostfrei_messaging_core::ConsumeError),
    #[error(transparent)]
    DomainConsumer(#[from] rostfrei_nats::DomainEventConsumerError),
    #[error(transparent)]
    DomainRegistration(#[from] rostfrei::DomainEventRegistrationError),
    #[error(transparent)]
    Fixture(#[from] rostfrei_fixtures::FixtureApplyError),
    #[error(transparent)]
    Observation(#[from] crate::ObservationError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Configuration(String),
    #[error("owned worker stopped: {0}")]
    Worker(String),
}

/// Explicit resource and subscription policy for one bounded-context runtime.
/// Command generations are independent of payload schema versions.
#[derive(Clone)]
pub struct NatsRuntimeConfig {
    pub context: BoundedContext,
    pub messaging: ApplicationMessagingConfig,
    pub event_store: NatsEventStoreConfig,
    pub commands: Vec<ConsumerConfig<CommandAddress>>,
    pub integrations: Vec<ConsumerConfig<IntegrationEventAddress>>,
    pub domain_events: NatsDomainEventConsumerConfig,
}

impl NatsRuntimeConfig {
    fn validate(&self) -> Result<(), NatsHostError> {
        let scope = self.context.traffic_scope();
        let application = self.context.application();
        if self.messaging.topology().application() != application
            || self.messaging.topology().traffic_scope() != scope
            || self.event_store.application() != application
            || self.event_store.traffic_scope() != scope
            || self.event_store.bounded_context() != self.context.name()
            || self.commands.iter().any(|c| {
                c.address().application() != application.as_str()
                    || c.address().traffic_scope() != scope
                    || c.address().context() != self.context.name().as_str()
            })
            || self.integrations.iter().any(|c| {
                c.address().application() != application.as_str()
                    || c.address().traffic_scope() != scope
                    || c.address().context() != self.context.name().as_str()
            })
        {
            return Err(NatsHostError::Configuration(
                "runtime resources must belong to its application, scope and context".into(),
            ));
        }
        Ok(())
    }

    pub fn new(
        context: BoundedContext,
        bindings: &ApplicationBindings,
        subscription_generation: u32,
    ) -> Result<Self, NatsHostError> {
        let messaging = ApplicationMessagingConfig::new_in_scope(
            context.application(),
            context.traffic_scope(),
        )?
        .with_max_bytes(64 * 1024 * 1024)?;
        let event_store = NatsEventStoreConfig::for_bounded_context(&context)?
            .with_storage_limits(256 * 1024 * 1024, 512 * 1024)?;
        let mut commands = Vec::new();
        for command in &bindings.commands {
            if command.bounded_context != context.name().as_str() {
                return Err(NatsHostError::Configuration(
                    "bindings must belong to the configured bounded context".into(),
                ));
            }
            let address = context.command_address(command.command_name)?;
            if commands
                .iter()
                .any(|c: &ConsumerConfig<CommandAddress>| c.address() == &address)
            {
                continue;
            }
            commands.push(ConsumerConfig::new(
                context.consumer_name(command.command_name, subscription_generation)?,
                context.durable_name(command.command_name, subscription_generation)?,
                address,
                Duration::from_secs(45),
                Duration::from_secs(30),
                1,
                5,
            )?);
        }
        let mut integrations = Vec::new();
        for (name, _) in &bindings.integration_handlers {
            let purpose = format!("{name}-consumer");
            integrations.push(ConsumerConfig::new(
                context.consumer_name(&purpose, 1)?,
                context.durable_name(&purpose, 1)?,
                context.integration_event_address(name)?,
                Duration::from_secs(45),
                Duration::from_secs(30),
                4,
                5,
            )?);
        }
        let domain_events = NatsDomainEventConsumerConfig::new(
            context.consumer_name("integration-publisher", 1)?,
            context.durable_name("integration-publisher", 1)?,
            Duration::from_secs(45),
            Duration::from_secs(30),
            RetryDelay::new(Duration::from_secs(1))?,
        )?;
        Ok(Self {
            context,
            messaging,
            event_store,
            commands,
            integrations,
            domain_events,
        })
    }

    pub async fn provision(&self, connection: &NatsConnection) -> Result<(), NatsHostError> {
        self.validate()?;
        provision_application_messaging(connection.jetstream(), &self.messaging).await?;
        provision_event_store(connection.jetstream(), &self.event_store).await?;
        for consumer in &self.commands {
            provision_durable_consumer(connection.jetstream(), self.messaging.topology(), consumer)
                .await?;
        }
        for consumer in &self.integrations {
            provision_durable_consumer(connection.jetstream(), self.messaging.topology(), consumer)
                .await?;
        }
        Ok(())
    }
}

struct ScopedTransport {
    inner: CommandBusTransport,
    gate: Arc<RwLock<()>>,
    available: Arc<AtomicBool>,
}
#[async_trait]
impl CommandTransport for ScopedTransport {
    fn maximum_payload_len(&self) -> usize {
        self.inner.maximum_payload_len()
    }
    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        let _scope = self.gate.read().await;
        if !self.available.load(Ordering::Acquire) {
            return Err(CommandTransportError::new(
                CommandTransportErrorKind::Unavailable,
                "runtime unavailable until startup or Test reset succeeds",
            ));
        }
        self.inner.invoke(invocation, observer).await
    }
}

type Worker = JoinHandle<Result<(), NatsHostError>>;

/// Owns NATS workers, fixture suppression, submission/reset coordination and observation.
/// `connect` verifies existing resources; it never provisions or resets them.
pub struct NatsRuntime {
    connection: NatsConnection,
    config: NatsRuntimeConfig,
    bindings: Arc<ApplicationBindings>,
    store: NatsEventStore,
    messaging: Arc<NatsMessagingAdapter>,
    transport: Arc<ScopedTransport>,
    gate: Arc<RwLock<()>>,
    available: Arc<AtomicBool>,
    fixture_events: Arc<RwLock<FixtureEventSet>>,
    reset_required: AtomicBool,
    workers: Mutex<Vec<Worker>>,
    observer: Mutex<Option<CorrelationObserver>>,
}

impl NatsRuntime {
    pub async fn provision(
        connection: NatsConnection,
        config: NatsRuntimeConfig,
        bindings: Arc<ApplicationBindings>,
    ) -> Result<Self, NatsHostError> {
        config.provision(&connection).await?;
        if !bindings.integration_mappings.is_empty() {
            provision_domain_event_consumer(
                connection.jetstream(),
                &config.event_store,
                &config.domain_events,
            )
            .await?;
        }
        Self::connect(connection, config, bindings).await
    }

    pub async fn connect(
        connection: NatsConnection,
        config: NatsRuntimeConfig,
        bindings: Arc<ApplicationBindings>,
    ) -> Result<Self, NatsHostError> {
        config.validate()?;
        connection
            .verify_application_messaging(&config.messaging)
            .await?;
        let store =
            NatsEventStore::connect(connection.jetstream().clone(), config.event_store.clone())
                .await?;
        let messaging = Arc::new(
            connection
                .messaging_adapter(config.messaging.topology().clone())
                .with_response_timeout(Duration::from_secs(30)),
        );
        let adapter: Arc<dyn CommandMessageAdapter> = messaging.clone();
        let gate = Arc::new(RwLock::new(()));
        let available = Arc::new(AtomicBool::new(false));
        let transport = Arc::new(ScopedTransport {
            inner: CommandBusTransport::new(CommandBus::new(config.context.clone(), adapter)),
            gate: gate.clone(),
            available: available.clone(),
        });
        Ok(Self {
            connection,
            config,
            bindings,
            store,
            messaging,
            transport,
            gate,
            available,
            fixture_events: Arc::new(RwLock::new(FixtureEventSet::default())),
            reset_required: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
            observer: Mutex::new(None),
        })
    }

    pub const fn config(&self) -> &NatsRuntimeConfig {
        &self.config
    }
    pub const fn store(&self) -> &NatsEventStore {
        &self.store
    }
    pub fn transport(&self) -> Arc<dyn CommandTransport> {
        self.transport.clone()
    }
    pub fn quarantine_reader(&self) -> Arc<dyn rostfrei_messaging_core::QuarantineReader> {
        Arc::new(rostfrei_nats::NatsQuarantineReader::new(
            self.connection.jetstream().clone(),
            self.config.messaging.topology().clone(),
        ))
    }
    pub async fn has_correlation_observer(&self) -> bool {
        self.available.load(Ordering::Acquire) && self.observer.lock().await.is_some()
    }

    pub async fn apply_fixture(
        &self,
        fixture: &Fixture,
    ) -> Result<FixtureApplyReport, NatsHostError> {
        let _scope = self.gate.write().await;
        self.apply_fixture_in_scope(fixture).await
    }

    async fn apply_fixture_in_scope(
        &self,
        fixture: &Fixture,
    ) -> Result<FixtureApplyReport, NatsHostError> {
        let plan = FixtureEventSet::new(std::slice::from_ref(fixture))?;
        let mut events = self.fixture_events.write().await;
        events.extend(plan);
        let result = self.bindings.fixtures.apply(&self.store, fixture).await;
        drop(events);
        Ok(result?)
    }

    pub async fn start_workers(&self) -> Result<(), NatsHostError> {
        let _scope = self.gate.write().await;
        if self.reset_required.load(Ordering::Acquire) {
            return Err(NatsHostError::Configuration(
                "Test remains unavailable until a successful reset".into(),
            ));
        }
        self.start_in_scope().await
    }

    async fn start_in_scope(&self) -> Result<(), NatsHostError> {
        let mut workers = self.workers.lock().await;
        if !workers.is_empty() {
            if workers.iter().all(|w| !w.is_finished()) {
                return Ok(());
            }
            return Err(NatsHostError::Worker(
                "a worker has exited; stop the runtime before restarting".into(),
            ));
        }
        // Prepare and verify everything before spawning any task.
        let factory = self
            .connection
            .consumer_factory(self.config.messaging.topology().clone());
        let store: Arc<dyn EventStore> = Arc::new(self.store.clone());
        let processor = self
            .bindings
            .command_processor(self.config.context.name().clone(), store)?;
        let handler: Arc<dyn MessageHandler<CommandAddress>> =
            Arc::new(self.messaging.command_handler(Arc::new(processor)));
        let mut commands = Vec::new();
        for config in &self.config.commands {
            factory.verify_consumer(config).await?;
            commands.push(factory.create(config.clone())?);
        }
        let mut integrations = Vec::new();
        if self.config.integrations.len() != self.bindings.integration_handlers.len() {
            return Err(NatsHostError::Configuration(
                "integration consumers and handlers differ".into(),
            ));
        }
        for (config, (_, handler)) in self
            .config
            .integrations
            .iter()
            .zip(&self.bindings.integration_handlers)
        {
            factory.verify_consumer(config).await?;
            integrations.push((factory.create(config.clone())?, handler.clone()));
        }
        let domain = if self.bindings.integration_mappings.is_empty() {
            None
        } else {
            let adapter: Arc<dyn IntegrationMessageAdapter> = self.messaging.clone();
            let bus = IntegrationEventBus::new(self.config.context.clone(), adapter);
            let mut dispatcher = DomainEventDispatcher::new();
            for register in &self.bindings.integration_mappings {
                register(bus.clone(), &mut dispatcher)?;
            }
            Some(
                NatsDomainEventConsumer::connect_with_fixture_events(
                    self.connection.jetstream().clone(),
                    self.config.event_store.clone(),
                    self.config.domain_events.clone(),
                    Arc::new(dispatcher),
                    self.fixture_events.clone(),
                )
                .await?,
            )
        };
        let observer = self.observer.lock().await.clone();
        let observation = match observer {
            Some(observer) => Some(self.prepare_observation(observer).await?),
            None => None,
        };
        for consumer in commands {
            let handler = handler.clone();
            workers.push(tokio::spawn(async move {
                consumer.run(handler).await.map_err(Into::into)
            }));
        }
        for (consumer, handler) in integrations {
            workers.push(tokio::spawn(async move {
                consumer.run(handler).await.map_err(Into::into)
            }));
        }
        if let Some(consumer) = domain {
            workers.push(tokio::spawn(async move {
                let (_keepalive, shutdown) = watch::channel(false);
                consumer
                    .run_until_shutdown(shutdown)
                    .await
                    .map_err(Into::into)
            }));
        }
        if let Some((subscription, handler)) = observation {
            workers.push(tokio::spawn(async move {
                subscription.run(handler).await.map_err(Into::into)
            }));
        }
        self.available.store(true, Ordering::Release);
        drop(workers);
        Ok(())
    }

    async fn prepare_observation(
        &self,
        observer: CorrelationObserver,
    ) -> Result<
        (
            rostfrei_nats::NatsCorrelationSubscription,
            Arc<observation::TracerCorrelationHandler>,
        ),
        NatsHostError,
    > {
        let source = observer.observation_source(self.config.context.name().as_str())?;
        let subscription = NatsCorrelationObserver::new_in_scope(
            self.connection.client().clone(),
            self.config.context.application().clone(),
            self.config.context.traffic_scope(),
            self.config.event_store.subject_filter(),
        )
        .with_streams(
            self.config.event_store.stream_name(),
            self.config
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .subscribe()
        .await?;
        // Both subscriptions are established before callers can publish. The
        // handler owns the guard through failures, cancellation and shutdown.
        if let Some(source) = &source {
            source.ready();
        }
        Ok((
            subscription,
            Arc::new(observation::TracerCorrelationHandler {
                observer,
                store: self.store.clone(),
                bindings: self.bindings.clone(),
                source,
            }),
        ))
    }

    pub async fn attach_observer(
        &self,
        observer: CorrelationObserver,
    ) -> Result<(), NatsHostError> {
        let _scope = self.gate.write().await;
        if self.reset_required.load(Ordering::Acquire) {
            return Err(NatsHostError::Configuration(
                "Test remains unavailable until a successful reset".into(),
            ));
        }
        self.stop_in_scope().await;
        *self.observer.lock().await = Some(observer);
        self.start_in_scope().await
    }

    /// Compatibility entry point for applications owning their own supervisor.
    /// The returned task owns source readiness/unavailability, including on abort.
    pub async fn start_correlation_observer(
        &self,
        observer: CorrelationObserver,
    ) -> Result<Worker, NatsHostError> {
        let (subscription, handler) = self.prepare_observation(observer).await?;
        Ok(tokio::spawn(async move {
            subscription.run(handler).await.map_err(Into::into)
        }))
    }

    pub async fn wait_for_worker_exit(&self) -> NatsHostError {
        loop {
            let finished = {
                let mut workers = self.workers.lock().await;
                workers
                    .iter()
                    .position(JoinHandle::is_finished)
                    .map(|index| workers.swap_remove(index))
            };
            if let Some(worker) = finished {
                self.available.store(false, Ordering::Release);
                return match worker.await {
                    Ok(Err(error)) => error,
                    result => NatsHostError::Worker(format!("{result:?}")),
                };
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub async fn stop_workers(&self) {
        let _scope = self.gate.write().await;
        self.stop_in_scope().await;
    }
    async fn stop_in_scope(&self) {
        self.available.store(false, Ordering::Release);
        let workers = std::mem::take(&mut *self.workers.lock().await);
        for worker in &workers {
            worker.abort();
        }
        for worker in workers {
            let _ = worker.await;
        }
    }

    async fn reset_scope(&self, fixture: &Fixture) -> Result<(), NatsHostError> {
        if self.config.context.traffic_scope() != TrafficScope::Test {
            return Err(NatsHostError::Configuration(
                "reset requires Test scope".into(),
            ));
        }
        let _scope = self.gate.write().await;
        self.reset_required.store(true, Ordering::Release);
        self.stop_in_scope().await;
        *self.fixture_events.write().await = FixtureEventSet::default();
        let topology = self.config.messaging.topology();
        for stream in [
            topology.command_stream().as_str(),
            topology.command_response_stream().as_str(),
            topology.integration_event_stream().as_str(),
            topology.quarantine_stream().as_str(),
            self.config.event_store.stream_name(),
        ] {
            self.connection.delete_stream_if_exists(stream).await?;
        }
        self.config.provision(&self.connection).await?;
        if !self.bindings.integration_mappings.is_empty() {
            provision_domain_event_consumer(
                self.connection.jetstream(),
                &self.config.event_store,
                &self.config.domain_events,
            )
            .await?;
        }
        self.apply_fixture_in_scope(fixture).await?;
        self.start_in_scope().await?;
        self.reset_required.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for NatsRuntime {
    fn drop(&mut self) {
        for worker in self.workers.get_mut().drain(..) {
            worker.abort();
        }
    }
}
#[async_trait]
impl TestScenarioReset for NatsRuntime {
    async fn reset(&self, fixture: &Fixture) -> Result<(), TestScenarioResetError> {
        self.reset_scope(fixture)
            .await
            .map_err(|e| TestScenarioResetError::Failed(e.to_string()))
    }
}

/// HTTP capability/listener configuration; enabling it never selects Normal state for Test.
pub struct TracerConfig {
    listener: tokio::net::TcpListener,
    http: HttpConfig,
    repository: Option<Arc<dyn TestRepository>>,
    payload_policy: Arc<dyn TracePayloadPolicy>,
    dispatch: Option<Arc<NatsRuntime>>,
    observation: std::collections::BTreeMap<crate::ObservationScope, Arc<crate::ObservationFeed>>,
}
impl TracerConfig {
    pub fn new(listener: tokio::net::TcpListener, http: HttpConfig) -> Self {
        Self {
            listener,
            http,
            repository: None,
            payload_policy: Arc::new(crate::RedactTracePayloads),
            dispatch: None,
            observation: std::collections::BTreeMap::new(),
        }
    }
    #[must_use]
    pub fn with_test_repository(mut self, repository: Arc<dyn TestRepository>) -> Self {
        self.repository = Some(repository);
        self
    }
    #[must_use]
    pub fn with_trace_payload_policy(mut self, policy: Arc<dyn TracePayloadPolicy>) -> Self {
        self.payload_policy = policy;
        self
    }
    /// Enable continuous observation for the matching application/scope runtime.
    /// Replaces any previously configured feed for that scope, like `TracerBuilder`.
    #[must_use]
    pub fn with_continuous_observation(mut self, feed: Arc<crate::ObservationFeed>) -> Self {
        self.observation.insert(feed.scope(), feed);
        self
    }
    /// The host supervises and stops this explicitly supplied Normal runtime too.
    #[must_use]
    pub fn with_dispatch_runtime(mut self, runtime: Arc<NatsRuntime>) -> Self {
        self.dispatch = Some(runtime);
        self
    }

    fn validate_observation(&self, test: &NatsRuntimeConfig) -> Result<(), NatsHostError> {
        for feed in self.observation.values() {
            let runtime = match feed.scope() {
                crate::ObservationScope::Test => test,
                crate::ObservationScope::Production => {
                    &self
                        .dispatch
                        .as_ref()
                        .ok_or_else(|| {
                            NatsHostError::Configuration(
                                "production observation requires a Normal dispatch runtime".into(),
                            )
                        })?
                        .config
                }
            };
            if feed.catalog().application != runtime.context.application().as_str() {
                return Err(NatsHostError::Configuration(
                    "observation feed must belong to its runtime's application".into(),
                ));
            }
        }
        Ok(())
    }
}

pub struct NatsApplicationBuilder {
    connection: NatsConnection,
    config: NatsRuntimeConfig,
    bindings: Arc<ApplicationBindings>,
    fixture: Option<Fixture>,
    initial_fixture: Option<Fixture>,
    fixtures: Vec<Fixture>,
    tracer: Option<TracerConfig>,
}
impl NatsApplicationBuilder {
    /// Explicit, idempotent initialization without deleting resources, also in Normal scope.
    #[must_use]
    pub fn with_initial_fixture(mut self, fixture: Fixture) -> Self {
        self.initial_fixture = Some(fixture);
        self
    }
    #[must_use]
    pub fn with_runtime_config(mut self, config: NatsRuntimeConfig) -> Self {
        self.config = config;
        self
    }
    #[must_use]
    pub fn with_default_test_fixture(mut self, fixture: Fixture) -> Self {
        self.fixture = Some(fixture);
        self
    }
    #[must_use]
    pub fn with_test_fixture(mut self, fixture: Fixture) -> Self {
        self.fixtures.push(fixture);
        self
    }
    #[must_use]
    pub fn with_tracer(mut self, config: Option<TracerConfig>) -> Self {
        self.tracer = config;
        self
    }

    pub async fn provision_and_start(self) -> Result<NatsApplication, NatsHostError> {
        self.start(true).await
    }
    pub async fn connect_and_start(self) -> Result<NatsApplication, NatsHostError> {
        self.start(false).await
    }

    #[allow(
        clippy::too_many_lines,
        reason = "startup preparation and rollback are kept together"
    )]
    async fn start(self, provision: bool) -> Result<NatsApplication, NatsHostError> {
        if (self.fixture.is_some() || self.tracer.is_some())
            && self.config.context.traffic_scope() != TrafficScope::Test
        {
            return Err(NatsHostError::Configuration("fixtures and Tracer require an explicitly isolated Test runtime; production dispatch is a separate attachment".into()));
        }
        if !provision && self.fixture.is_some() {
            return Err(NatsHostError::Configuration("connect does not reset fixtures; use explicit provision_and_start for the Test host".into()));
        }
        if let Some(tracer) = &self.tracer {
            tracer.validate_observation(&self.config)?;
        }
        let runtime = Arc::new(if provision {
            NatsRuntime::provision(self.connection, self.config, self.bindings.clone()).await?
        } else {
            NatsRuntime::connect(self.connection, self.config, self.bindings.clone()).await?
        });
        let dispatch = self.tracer.as_ref().and_then(|c| c.dispatch.clone());
        let (stop_tasks, task_shutdown) = watch::channel(false);
        let result = async {
            if let Some(fixture) = &self.initial_fixture {
                runtime.apply_fixture(fixture).await?;
            }
            if let Some(fixture) = &self.fixture {
                runtime.reset_scope(fixture).await?;
            } else {
                runtime.start_workers().await?;
            }
            let mut tasks = Vec::new();
            let tracer = if let Some(config) = self.tracer {
                let fixture = self.fixture.ok_or_else(|| {
                    NatsHostError::Configuration("Tracer requires a default Test fixture".into())
                })?;
                let store = Arc::new(runtime.store.clone());
                let mut builder = self
                    .bindings
                    .tracer_builder(store.clone())
                    .with_test_event_store(store.clone())
                    .with_stream_directory(store)
                    .with_test_transport(runtime.transport())
                    .with_test_quarantine_reader(runtime.quarantine_reader())
                    .with_test_scenario_reset(runtime.clone())
                    .with_default_test_fixture(fixture)
                    .with_trace_payload_policy(config.payload_policy);
                for feed in config.observation.into_values() {
                    builder = builder.with_continuous_observation(feed);
                }
                for fixture in self.fixtures {
                    builder = builder.with_test_fixture(fixture);
                }
                if let Some(repository) = config.repository {
                    builder = builder.with_test_repository(repository);
                }
                if let Some(dispatch) = &dispatch {
                    if dispatch.config.context.traffic_scope() != TrafficScope::Normal {
                        return Err(NatsHostError::Configuration(
                            "dispatch requires Normal scope".into(),
                        ));
                    }
                    builder = builder
                        .with_dispatch_transport(dispatch.transport())
                        .with_production_quarantine_reader(dispatch.quarantine_reader());
                }
                let tracer = builder.build()?;
                runtime
                    .attach_observer(tracer.correlation_observer(OperationMode::Test))
                    .await?;
                if let Some(dispatch) = &dispatch {
                    dispatch
                        .attach_observer(tracer.correlation_observer(OperationMode::Dispatch))
                        .await?;
                }
                let app = shutdown_aware_router(
                    http::router(tracer.clone(), config.http),
                    task_shutdown.clone(),
                );
                let mut shutdown = task_shutdown.clone();
                tasks.push(tokio::spawn(async move {
                    axum::serve(config.listener, app)
                        .with_graceful_shutdown(async move {
                            let _ = shutdown.changed().await;
                        })
                        .await
                        .map_err(Into::into)
                }));
                Some(tracer)
            } else {
                None
            };
            let watched = runtime.clone();
            let mut shutdown = task_shutdown.clone();
            tasks.push(tokio::spawn(async move {
                tokio::select! {
                    error = watched.wait_for_worker_exit() => Err(error),
                    _ = shutdown.changed() => Ok(()),
                }
            }));
            if let Some(dispatch) = &dispatch {
                let watched = dispatch.clone();
                let mut shutdown = task_shutdown.clone();
                tasks.push(tokio::spawn(async move {
                    tokio::select! {
                        error = watched.wait_for_worker_exit() => Err(error),
                        _ = shutdown.changed() => Ok(()),
                    }
                }));
            }
            Ok((tracer, tasks))
        }
        .await;
        match result {
            Ok((tracer, tasks)) => Ok(NatsApplication::supervise(
                runtime, dispatch, tracer, tasks, stop_tasks,
            )),
            Err(error) => {
                runtime.stop_workers().await;
                if let Some(dispatch) = dispatch {
                    dispatch.stop_workers().await;
                }
                Err(error)
            }
        }
    }
}

/// A running application. Call `run_until` to supervise it and always shut down.
/// Dropping it cancels owned HTTP/supervision tasks; `shutdown` awaits cleanup.
pub struct NatsApplication {
    runtime: Arc<NatsRuntime>,
    tracer: Option<Tracer>,
    shutdown: watch::Sender<bool>,
    supervisor: Option<Worker>,
}
impl NatsApplication {
    fn supervise(
        runtime: Arc<NatsRuntime>,
        dispatch: Option<Arc<NatsRuntime>>,
        tracer: Option<Tracer>,
        tasks: Vec<Worker>,
        stop_tasks: watch::Sender<bool>,
    ) -> Self {
        let (shutdown, mut receiver) = watch::channel(false);
        let watched = runtime.clone();
        let supervisor = tokio::spawn(async move {
            let mut tasks = OwnedTasks(tasks);
            let mut result = loop {
                tokio::select! {
                    _ = receiver.changed() => break Ok(()),
                    () = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
                if let Some(index) = tasks.0.iter().position(JoinHandle::is_finished) {
                    break match tasks.0.swap_remove(index).await {
                        Ok(Err(error)) => Err(error),
                        result => Err(NatsHostError::Worker(format!("{result:?}"))),
                    };
                }
            };
            let _ = stop_tasks.send(true);
            for mut task in tasks.0.drain(..) {
                let stopped = match tokio::time::timeout(Duration::from_secs(45), &mut task).await {
                    Ok(Ok(outcome)) => outcome,
                    Ok(Err(error)) => Err(NatsHostError::Worker(error.to_string())),
                    Err(_) => {
                        task.abort();
                        let _ = task.await;
                        Err(NatsHostError::Worker(
                            "HTTP shutdown exceeded 45 seconds".into(),
                        ))
                    }
                };
                if result.is_ok() {
                    result = stopped;
                }
            }
            watched.stop_workers().await;
            if let Some(dispatch) = dispatch {
                dispatch.stop_workers().await;
            }
            result
        });
        Self {
            runtime,
            tracer,
            shutdown,
            supervisor: Some(supervisor),
        }
    }
    pub fn builder(
        context: BoundedContext,
        connection: NatsConnection,
        bindings: Arc<ApplicationBindings>,
    ) -> Result<NatsApplicationBuilder, NatsHostError> {
        let config = NatsRuntimeConfig::new(context, &bindings, 1)?;
        Ok(NatsApplicationBuilder {
            connection,
            config,
            bindings,
            fixture: None,
            initial_fixture: None,
            fixtures: Vec::new(),
            tracer: None,
        })
    }
    pub const fn runtime(&self) -> &Arc<NatsRuntime> {
        &self.runtime
    }
    pub const fn tracer(&self) -> Option<&Tracer> {
        self.tracer.as_ref()
    }
    pub async fn run_until(
        mut self,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<(), NatsHostError> {
        let Some(mut supervisor) = self.supervisor.take() else {
            return Ok(());
        };
        tokio::select! {
            () = shutdown => { let _ = self.shutdown.send(true); supervisor.await.map_err(|e| NatsHostError::Worker(e.to_string()))? },
            result = &mut supervisor => result.map_err(|e| NatsHostError::Worker(e.to_string()))?,
        }
    }
    pub async fn shutdown(mut self) -> Result<(), NatsHostError> {
        let _ = self.shutdown.send(true);
        if let Some(supervisor) = self.supervisor.take() {
            supervisor
                .await
                .map_err(|e| NatsHostError::Worker(e.to_string()))??;
        }
        Ok(())
    }
}
impl Drop for NatsApplication {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}
struct OwnedTasks(Vec<Worker>);
impl Drop for OwnedTasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

// End streaming observation responses too, so an open SSE client cannot keep
// the HTTP server (and its Tracer state) alive after host shutdown.
fn shutdown_aware_router(router: axum::Router, shutdown: watch::Receiver<bool>) -> axum::Router {
    use futures_util::StreamExt as _;
    router.layer(axum::middleware::map_response(
        move |response: axum::response::Response| {
            let mut shutdown = shutdown.clone();
            async move {
                let (parts, body) = response.into_parts();
                let stream = body.into_data_stream().take_until(async move {
                    if !*shutdown.borrow_and_update() {
                        let _ = shutdown.changed().await;
                    }
                });
                axum::response::Response::from_parts(parts, axum::body::Body::from_stream(stream))
            }
        },
    ))
}
