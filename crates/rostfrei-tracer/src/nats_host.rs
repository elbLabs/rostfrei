//! Explicitly provisioned NATS runtime and optional, application-linked Tracer host.
//!
//! Test resources belong to the supplied application namespace. Until run identity
//! (#68) is available, use a dedicated broker or unique application namespace per
//! independent host. The reset gate coordinates this runtime's submissions only.

mod backend;
mod group;
mod observation;
mod retained_history;
mod routing;
pub use backend::NatsExecutionProfile;
use backend::{Backend, CurrentBackend};
use group::TestRuntimeGroup;
use routing::RuntimeGroup;

use crate::{
    ApplicationBindings, CommandBusTransport, CommandInvocation, CommandReceipt, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
    CorrelationObserver, OperationMode, TestRepository, TestScenarioReset, TestScenarioResetError,
    TracePayloadPolicy, Tracer,
    http::{self, HttpConfig},
};
use async_trait::async_trait;
use rostfrei_fixtures::{Fixture, FixtureApplyReport, FixtureEventSet};
use rostfrei_messaging_core::{
    BoundedContext, CommandAddress, ConsumerConfig, IntegrationEventAddress,
    MessageConsumerFactory, MessageHandler, RetryDelay, TrafficScope,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnection, NatsDomainEventConsumer,
    NatsDomainEventConsumerConfig, NatsEventStore, NatsEventStoreConfig, NatsMessagingAdapter,
    provision_application_messaging, provision_domain_event_consumer, provision_durable_consumer,
    provision_event_store,
};
use rostfrei_runtime::{
    CommandBus, CommandMessageAdapter, DomainEventDispatcher, IntegrationEventBus,
    IntegrationMessageAdapter,
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
    Processor(#[from] rostfrei_runtime::CommandProcessorError),
    #[error(transparent)]
    Registration(#[from] crate::RuntimeRegistrationError),
    #[error(transparent)]
    Contract(#[from] rostfrei_messaging_core::ContractError),
    #[error(transparent)]
    Nats(#[from] rostfrei_nats::NatsError),
    #[error(transparent)]
    Store(#[from] rostfrei_runtime::EventStoreError),
    #[error(transparent)]
    Consumer(#[from] rostfrei_messaging_core::ConsumeError),
    #[error(transparent)]
    DomainConsumer(#[from] rostfrei_nats::DomainEventConsumerError),
    #[error(transparent)]
    DomainRegistration(#[from] rostfrei_runtime::DomainEventRegistrationError),
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
    pub execution_profile: NatsExecutionProfile,
    pub context: BoundedContext,
    pub messaging: ApplicationMessagingConfig,
    pub event_store: NatsEventStoreConfig,
    pub commands: Vec<ConsumerConfig<CommandAddress>>,
    pub integrations: Vec<ConsumerConfig<IntegrationEventAddress>>,
    /// Retry policy for typed integration-event-to-command handlers.
    pub integration_retry_delay: RetryDelay,
    pub domain_events: NatsDomainEventConsumerConfig,
    domain_events_required: bool,
}

impl NatsRuntimeConfig {
    pub fn with_fresh_outcomes(mut self) -> Result<Self, NatsHostError> {
        self.event_store = self.event_store.with_fresh_outcomes()?;
        Ok(self)
    }

    /// Explicitly select a new writable split authority. This never migrates an
    /// existing history or excludes old writers on the operator's behalf.
    pub fn with_fresh_completed_outcome_epoch(
        mut self,
        epoch: &str,
    ) -> Result<Self, NatsHostError> {
        self.event_store = self.event_store.with_fresh_split_outcome_epoch(epoch)?;
        Ok(self)
    }

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
                    || c.name().context() != self.context.name().as_str()
            })
        {
            return Err(NatsHostError::Configuration(
                "runtime resources must belong to its application, scope and context".into(),
            ));
        }
        if self.event_store.outcome_state_stream_name().is_empty() {
            return Err(NatsHostError::Configuration(format!(
                "writable runtime for context `{}` requires explicit split outcome authority",
                self.context.name().as_str()
            )));
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
        for binding in &bindings.integration_handlers {
            let address = binding.address(&context)?;
            // The durable is derived from the address alone; competing handlers
            // on it would each quarantine the deliveries meant for the other.
            if integrations
                .iter()
                .any(|c: &ConsumerConfig<IntegrationEventAddress>| c.address() == &address)
            {
                return Err(NatsHostError::Configuration(format!(
                    "multiple integration handlers use the same address `{address}`"
                )));
            }
            let purpose = if address.context() == context.name().as_str() {
                format!("{}-consumer", binding.name)
            } else {
                // Foreign producer names may overlap. Frame the producer/event
                // identity through its canonical address, independently of schema.
                rostfrei_core::ContentFingerprint::digest(address.as_str().as_bytes()).to_hex()
            };
            integrations.push(ConsumerConfig::new(
                context.consumer_name(&purpose, 1)?,
                context.durable_name(&purpose, 1)?,
                address,
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
            execution_profile: NatsExecutionProfile::TrustedCompatibility,
            context,
            messaging,
            event_store,
            commands,
            integrations,
            integration_retry_delay: RetryDelay::new(Duration::from_secs(1))?,
            domain_events,
            domain_events_required: !bindings.integration_mappings.is_empty(),
        })
    }

    pub async fn provision(&self, connection: &NatsConnection) -> Result<(), NatsHostError> {
        self.validate()?;
        provision_application_messaging(connection.jetstream(), &self.messaging).await?;
        self.provision_context(connection).await
    }

    async fn provision_context(&self, connection: &NatsConnection) -> Result<(), NatsHostError> {
        provision_event_store(connection.jetstream(), &self.event_store).await?;
        if self.domain_events_required {
            provision_domain_event_consumer(
                connection.jetstream(),
                &self.event_store,
                &self.domain_events,
            )
            .await?;
        }
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

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    #[allow(
        clippy::panic_in_result_fn,
        reason = "test assertions report contract failures"
    )]
    fn writable_runtime_requires_split_outcome_authority() -> Result<(), NatsHostError> {
        let context = rostfrei_runtime::ApplicationName::new("host-config")?
            .test_bounded_context("counter")?;
        let config = NatsRuntimeConfig::new(context, &ApplicationBindings::new(), 1)?;
        assert!(matches!(
            config.validate(),
            Err(NatsHostError::Configuration(message)) if message.contains("split outcome authority")
        ));
        config.with_fresh_outcomes()?.validate()
    }
}

struct ScopedTransport {
    backend: CurrentBackend,
    gate: Arc<RwLock<()>>,
    available: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
}
impl ScopedTransport {
    fn ensure_available(&self) -> Result<(), CommandTransportError> {
        if self.available.load(Ordering::Acquire) && !self.shutting_down.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(CommandTransportError::new(
                CommandTransportErrorKind::Unavailable,
                "runtime unavailable during startup, stop, shutdown or failed Test reset",
            ))
        }
    }
}
#[async_trait]
impl CommandTransport for ScopedTransport {
    fn maximum_payload_len(&self) -> usize {
        self.backend.snapshot().transport.maximum_payload_len()
    }
    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        // Reject immediately while teardown waits for already admitted readers.
        self.ensure_available()?;
        let _scope = self.gate.read().await;
        self.ensure_available()?;
        self.backend
            .snapshot()
            .transport
            .invoke(invocation, observer)
            .await
    }
}

type Worker = JoinHandle<Result<(), NatsHostError>>;
type WorkerFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NatsHostError>> + Send>>;

#[derive(Default)]
struct PreparedWorkers {
    application: Vec<WorkerFuture>,
    observation: Option<WorkerFuture>,
}

impl PreparedWorkers {
    fn launch(self, runtime: &NatsRuntime, workers: &mut RuntimeWorkers) {
        workers
            .application
            .extend(self.application.into_iter().map(tokio::spawn));
        workers.observation = self.observation.map(tokio::spawn);
        runtime.available.store(true, Ordering::Release);
    }
}

#[derive(Default)]
struct RuntimeWorkers {
    application: Vec<Worker>,
    observation: Option<Worker>,
}

impl RuntimeWorkers {
    const fn is_empty(&self) -> bool {
        self.application.is_empty() && self.observation.is_none()
    }

    fn iter(&self) -> impl Iterator<Item = &Worker> {
        self.application.iter().chain(self.observation.iter())
    }

    fn take_finished(&mut self) -> Option<Worker> {
        if let Some(index) = self.application.iter().position(JoinHandle::is_finished) {
            Some(self.application.swap_remove(index))
        } else if self
            .observation
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            self.observation.take()
        } else {
            None
        }
    }
}

/// Owns NATS workers, fixture suppression, submission/reset coordination and observation.
/// `connect` verifies existing resources; it never provisions or resets them.
pub struct NatsRuntime {
    connection: NatsConnection,
    config: NatsRuntimeConfig,
    bindings: Arc<ApplicationBindings>,
    backend: CurrentBackend,
    transport: Arc<ScopedTransport>,
    gate: Arc<RwLock<()>>,
    available: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
    fixture_events: Arc<RwLock<FixtureEventSet>>,
    reset_required: AtomicBool,
    // A member cannot independently delete streams shared with other contexts.
    grouped_reset: bool,
    workers: Mutex<RuntimeWorkers>,
    observer: Mutex<Option<(CorrelationObserver, bool)>>,
}

impl NatsRuntime {
    pub async fn provision(
        connection: NatsConnection,
        config: NatsRuntimeConfig,
        bindings: Arc<ApplicationBindings>,
    ) -> Result<Self, NatsHostError> {
        Self::validate_resource_plan(&config, &bindings)?;
        config.provision(&connection).await?;
        Self::connect(connection, config, bindings).await
    }

    pub async fn connect(
        connection: NatsConnection,
        config: NatsRuntimeConfig,
        bindings: Arc<ApplicationBindings>,
    ) -> Result<Self, NatsHostError> {
        Self::validate_resource_plan(&config, &bindings)?;
        connection
            .verify_application_messaging(&config.messaging)
            .await?;
        let store =
            NatsEventStore::connect(connection.jetstream().clone(), config.event_store.clone())
                .await?;
        let backend = CurrentBackend::new(Backend::new(&connection, &config, &bindings, store)?);
        let gate = Arc::new(RwLock::new(()));
        let available = Arc::new(AtomicBool::new(false));
        let shutting_down = Arc::new(AtomicBool::new(false));
        let transport = Arc::new(ScopedTransport {
            backend: backend.clone(),
            gate: gate.clone(),
            available: available.clone(),
            shutting_down: shutting_down.clone(),
        });
        Ok(Self {
            connection,
            config,
            bindings,
            backend,
            transport,
            gate,
            available,
            shutting_down,
            fixture_events: Arc::new(RwLock::new(FixtureEventSet::default())),
            reset_required: AtomicBool::new(false),
            grouped_reset: false,
            workers: Mutex::new(RuntimeWorkers::default()),
            observer: Mutex::new(None),
        })
    }

    pub const fn config(&self) -> &NatsRuntimeConfig {
        &self.config
    }

    fn validate_resource_plan(
        config: &NatsRuntimeConfig,
        bindings: &ApplicationBindings,
    ) -> Result<(), NatsHostError> {
        config.validate()?;
        let expected_commands = bindings
            .commands
            .iter()
            .map(|command| {
                if command.bounded_context != config.context.name().as_str() {
                    return Err(NatsHostError::Configuration(
                        "bindings must belong to the configured bounded context".into(),
                    ));
                }
                Ok(config.context.command_address(command.command_name)?)
            })
            .collect::<Result<std::collections::HashSet<_>, NatsHostError>>()?;
        let configured_commands = config
            .commands
            .iter()
            .map(ConsumerConfig::address)
            .collect::<std::collections::HashSet<_>>();
        if expected_commands.len() != configured_commands.len()
            || configured_commands.len() != config.commands.len()
            || !configured_commands
                .iter()
                .all(|address| expected_commands.contains(*address))
        {
            return Err(NatsHostError::Configuration(
                "command consumers must match registered command addresses".into(),
            ));
        }
        if config.integrations.len() != bindings.integration_handlers.len() {
            return Err(NatsHostError::Configuration(
                "integration consumers and handlers differ".into(),
            ));
        }
        for (consumer, handler) in config
            .integrations
            .iter()
            .zip(&bindings.integration_handlers)
        {
            if consumer.address() != &handler.address(&config.context)? {
                return Err(NatsHostError::Configuration(
                    "integration subscription must match its typed producer contract".into(),
                ));
            }
        }
        if config.domain_events_required == bindings.integration_mappings.is_empty() {
            return Err(NatsHostError::Configuration(
                "runtime resource plan must match its registered integration mappings".into(),
            ));
        }
        Ok(())
    }
    /// Owned snapshot of the current connected authority. Retained snapshots are
    /// intentionally fenced after reset; request a new snapshot for new work.
    pub fn store(&self) -> NatsEventStore {
        self.backend.snapshot().store.clone()
    }
    /// Compose a Tracer whose history and directory follow this runtime's current
    /// Test backend across resets, while diagnostics remain history-only.
    pub fn tracer_builder(&self) -> crate::TracerBuilder {
        let current = Arc::new(self.backend.clone());
        let builder = self
            .bindings
            .tracer_builder(current.clone())
            .with_stream_directory(current.clone());
        if self.config.context.traffic_scope() == TrafficScope::Test {
            builder.with_test_event_store(current)
        } else {
            builder
        }
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
        self.available.load(Ordering::Acquire)
            && self
                .workers
                .lock()
                .await
                .observation
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
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
        if self.config.context.traffic_scope() != TrafficScope::Test {
            return Err(NatsHostError::Configuration(
                "fixture injection requires Test scope".into(),
            ));
        }
        let plan = FixtureEventSet::new(std::slice::from_ref(fixture))?;
        let mut events = self.fixture_events.write().await;
        events.extend(plan);
        let result = self.bindings.fixtures.apply(&self.store(), fixture).await;
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
            if self.available.load(Ordering::Acquire)
                && !self.shutting_down.load(Ordering::Acquire)
                && workers.iter().all(|w| !w.is_finished())
            {
                return Ok(());
            }
            drop(workers);
            return Err(NatsHostError::Worker(
                "a worker has exited; stop the runtime before restarting".into(),
            ));
        }
        self.prepare_workers().await?.launch(self, &mut workers);
        drop(workers);
        Ok(())
    }

    async fn prepare_workers(&self) -> Result<PreparedWorkers, NatsHostError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(NatsHostError::Configuration(
                "runtime is shutting down".into(),
            ));
        }
        // Prepare and verify everything before spawning any task.
        let factory = self
            .connection
            .consumer_factory(self.config.messaging.topology().clone());
        let backend = self.backend.snapshot();
        let handler = backend.handler.clone();
        let mut prepared = PreparedWorkers::default();
        for config in &self.config.commands {
            factory.verify_consumer(config).await?;
            let consumer = factory.create(config.clone())?;
            let handler = handler.clone();
            prepared.application.push(Box::pin(async move {
                consumer.run(handler).await.map_err(Into::into)
            }));
        }
        for (config, binding) in self
            .config
            .integrations
            .iter()
            .zip(&self.bindings.integration_handlers)
        {
            factory.verify_consumer(config).await?;
            let adapter: Arc<dyn CommandMessageAdapter> = backend.messaging.clone();
            let bus = CommandBus::new(self.config.context.clone(), adapter);
            let handler = (binding.handler)(
                bus,
                config.durable_name().clone(),
                self.config.integration_retry_delay,
            );
            let consumer = factory.create(config.clone())?;
            prepared.application.push(Box::pin(async move {
                consumer.run(handler).await.map_err(Into::into)
            }));
        }
        if self.config.domain_events_required {
            let adapter: Arc<dyn IntegrationMessageAdapter> = backend.messaging.clone();
            let bus = IntegrationEventBus::new(self.config.context.clone(), adapter);
            let mut dispatcher = DomainEventDispatcher::with_codecs(self.bindings.codecs.clone());
            for register in &self.bindings.integration_mappings {
                register(bus.clone(), &mut dispatcher)?;
            }
            let consumer = NatsDomainEventConsumer::connect_with_fixture_events(
                self.connection.jetstream().clone(),
                self.config.event_store.clone(),
                self.config.domain_events.clone(),
                Arc::new(dispatcher),
                self.fixture_events.clone(),
            )
            .await?;
            prepared.application.push(Box::pin(async move {
                let (_keepalive, shutdown) = watch::channel(false);
                consumer
                    .run_until_shutdown(shutdown)
                    .await
                    .map_err(Into::into)
            }));
        }
        let observer = self.observer.lock().await.clone();
        if let Some((observer, producer_only)) = observer {
            let (subscription, handler) =
                observation::prepare(self, observer, producer_only).await?;
            prepared.observation = Some(Box::pin(async move {
                subscription.run(handler).await.map_err(Into::into)
            }));
        }
        Ok(prepared)
    }

    /// Attaches or replaces observation without interrupting application work.
    /// A failed replacement leaves observation unattached; application workers
    /// keep their deliveries and the caller can retry attachment explicitly.
    pub async fn attach_observer(
        &self,
        observer: CorrelationObserver,
    ) -> Result<(), NatsHostError> {
        let _scope = self.gate.write().await;
        self.attach_observer_in_scope(observer, false).await
    }

    async fn attach_observer_in_scope(
        &self,
        observer: CorrelationObserver,
        producer_only: bool,
    ) -> Result<(), NatsHostError> {
        if self.reset_required.load(Ordering::Acquire) {
            return Err(NatsHostError::Configuration(
                "Test remains unavailable until a successful reset".into(),
            ));
        }
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(NatsHostError::Configuration(
                "runtime is shutting down".into(),
            ));
        }
        let mut workers = self.workers.lock().await;
        if workers.is_empty() {
            *self.observer.lock().await = Some((observer, producer_only));
            drop(workers);
            return self.start_in_scope().await;
        }
        if !self.available.load(Ordering::Acquire) || workers.iter().any(JoinHandle::is_finished) {
            return Err(NatsHostError::Worker(
                "a worker has exited; stop the runtime before restarting".into(),
            ));
        }
        // Release the old source guard before registering the same source again.
        let mut configured = self.observer.lock().await;
        *configured = None;
        if let Some(worker) = workers.observation.take() {
            worker.abort();
            let _ = worker.await;
        }
        let (subscription, handler) =
            observation::prepare(self, observer.clone(), producer_only).await?;
        let worker =
            tokio::spawn(async move { subscription.run(handler).await.map_err(Into::into) });
        *configured = Some((observer, producer_only));
        drop(configured);
        workers.observation = Some(worker);
        drop(workers);
        Ok(())
    }

    /// Compatibility entry point for applications owning their own supervisor.
    /// The returned task owns source readiness/unavailability, including on abort.
    pub async fn start_correlation_observer(
        &self,
        observer: CorrelationObserver,
    ) -> Result<Worker, NatsHostError> {
        let (subscription, handler) = observation::prepare(self, observer, false).await?;
        Ok(tokio::spawn(async move {
            subscription.run(handler).await.map_err(Into::into)
        }))
    }

    pub async fn wait_for_worker_exit(&self) -> NatsHostError {
        loop {
            let finished = self.workers.lock().await.take_finished();
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
        self.available.store(false, Ordering::Release);
        let _scope = self.gate.write().await;
        self.stop_in_scope().await;
    }

    fn begin_shutdown(&self) {
        // Permanent for this runtime: even an overlapping reset cannot reopen
        // a retained sender after its owning application starts shutdown.
        self.shutting_down.store(true, Ordering::Release);
        self.available.store(false, Ordering::Release);
    }
    async fn stop_in_scope(&self) {
        self.available.store(false, Ordering::Release);
        let workers = std::mem::take(&mut *self.workers.lock().await);
        for worker in workers.iter() {
            worker.abort();
        }
        for worker in workers.application.into_iter().chain(workers.observation) {
            let _ = worker.await;
        }
    }

    async fn reset_scope(&self, fixture: &Fixture) -> Result<(), NatsHostError> {
        if self.config.context.traffic_scope() != TrafficScope::Test {
            return Err(NatsHostError::Configuration(
                "reset requires Test scope".into(),
            ));
        }
        if self.grouped_reset {
            return Err(NatsHostError::Configuration(
                "a hosted context must reset through its application's Test scenario".into(),
            ));
        }
        let _scope = self.gate.write().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(NatsHostError::Configuration(
                "runtime is shutting down".into(),
            ));
        }
        self.reset_required.store(true, Ordering::Release);
        self.stop_in_scope().await;
        *self.fixture_events.write().await = FixtureEventSet::default();
        self.delete_shared_streams().await?;
        self.delete_private_streams().await?;
        self.config.provision(&self.connection).await?;
        self.replace_backend(self.connect_backend().await?);
        self.apply_fixture_in_scope(fixture).await?;
        self.start_in_scope().await?;
        self.reset_required.store(false, Ordering::Release);
        Ok(())
    }

    async fn delete_shared_streams(&self) -> Result<(), NatsHostError> {
        let topology = self.config.messaging.topology();
        for stream in [
            topology.command_stream().as_str(),
            topology.command_response_stream().as_str(),
            topology.integration_event_stream().as_str(),
            topology.quarantine_stream().as_str(),
        ] {
            self.connection.delete_stream_if_exists(stream).await?;
        }
        Ok(())
    }

    async fn delete_private_streams(&self) -> Result<(), NatsHostError> {
        for stream in group::private_stream_names(&self.config) {
            self.connection.delete_stream_if_exists(stream).await?;
        }
        Ok(())
    }

    async fn connect_backend(&self) -> Result<Backend, NatsHostError> {
        let store = NatsEventStore::connect(
            self.connection.jetstream().clone(),
            self.config.event_store.clone(),
        )
        .await?;
        Backend::new(&self.connection, &self.config, &self.bindings, store)
    }

    fn replace_backend(&self, backend: Backend) {
        self.backend.replace(backend);
    }
}

impl Drop for NatsRuntime {
    fn drop(&mut self) {
        self.begin_shutdown();
        for worker in self.workers.get_mut().iter() {
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

/// HTTP capabilities, listener and observation feeds for the optional Tracer.
/// Enabling it never selects Normal state for Test.
pub struct TracerConfig {
    listener: tokio::net::TcpListener,
    http: HttpConfig,
    repository: Option<Arc<dyn TestRepository>>,
    payload_policy: Arc<dyn TracePayloadPolicy>,
    dispatch: Vec<Arc<NatsRuntime>>,
    production_event_stores: Vec<NatsEventStoreConfig>,
    observation: std::collections::BTreeMap<crate::ObservationScope, Arc<crate::ObservationFeed>>,
}
impl TracerConfig {
    pub fn new(listener: tokio::net::TcpListener, http: HttpConfig) -> Self {
        Self {
            listener,
            http,
            repository: None,
            payload_policy: Arc::new(crate::RedactTracePayloads),
            dispatch: Vec::new(),
            production_event_stores: Vec::new(),
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
    /// Production requires `with_dispatch_runtime`; feed/runtime applications must match.
    #[must_use]
    pub fn with_continuous_observation(mut self, feed: Arc<crate::ObservationFeed>) -> Self {
        self.observation.insert(feed.scope(), feed);
        self
    }
    /// Add a Normal context runtime. The host supervises and stops all attached runtimes.
    #[must_use]
    pub fn with_dispatch_runtime(mut self, runtime: Arc<NatsRuntime>) -> Self {
        self.dispatch.push(runtime);
        self
    }

    /// Adds an existing Normal store for inspection without provisioning or starting workers.
    /// The dispatch runtime's store is registered automatically.
    #[must_use]
    pub fn with_production_event_store(mut self, config: NatsEventStoreConfig) -> Self {
        self.production_event_stores.push(config);
        self
    }

    fn validate_observation(&self, test: &NatsRuntimeConfig) -> Result<(), NatsHostError> {
        for feed in self.observation.values() {
            let runtime = match feed.scope() {
                crate::ObservationScope::Test => test,
                crate::ObservationScope::Production => {
                    &self
                        .dispatch
                        .first()
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
    contexts: Vec<(NatsRuntimeConfig, Arc<ApplicationBindings>)>,
    fixture: Option<Fixture>,
    initial_fixture: Option<Fixture>,
    fixtures: Vec<Fixture>,
    tracer: Option<TracerConfig>,
}
impl NatsApplicationBuilder {
    pub fn with_fresh_outcomes(mut self) -> Result<Self, NatsHostError> {
        self.config = self.config.with_fresh_outcomes()?;
        Ok(self)
    }

    pub fn with_fresh_completed_outcome_epoch(
        mut self,
        epoch: &str,
    ) -> Result<Self, NatsHostError> {
        self.config = self.config.with_fresh_completed_outcome_epoch(epoch)?;
        Ok(self)
    }

    /// Explicit, idempotent Test initialization without deleting resources.
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
    /// Add a context in the same application and scope as the primary runtime.
    #[must_use]
    pub fn with_context(
        mut self,
        config: NatsRuntimeConfig,
        bindings: Arc<ApplicationBindings>,
    ) -> Self {
        self.contexts.push((config, bindings));
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
        Box::pin(self.start(true)).await
    }
    pub async fn connect_and_start(self) -> Result<NatsApplication, NatsHostError> {
        Box::pin(self.start(false)).await
    }

    fn validate(&self, provision: bool) -> Result<Vec<NatsEventStoreConfig>, NatsHostError> {
        if (self.fixture.is_some()
            || self.initial_fixture.is_some()
            || !self.fixtures.is_empty()
            || self.tracer.is_some())
            && self.config.context.traffic_scope() != TrafficScope::Test
        {
            return Err(NatsHostError::Configuration("fixtures and Tracer require an explicitly isolated Test runtime; production dispatch is a separate attachment".into()));
        }
        if !provision && self.fixture.is_some() {
            return Err(NatsHostError::Configuration("connect does not reset fixtures; use explicit provision_and_start for the Test host".into()));
        }
        NatsRuntime::validate_resource_plan(&self.config, &self.bindings)?;
        for (config, bindings) in &self.contexts {
            NatsRuntime::validate_resource_plan(config, bindings)?;
        }
        group::validate_scope(
            &self.config,
            std::iter::once(&self.config).chain(self.contexts.iter().map(|(config, _)| config)),
        )?;
        if let Some(tracer) = &self.tracer {
            if self.fixture.is_none() {
                return Err(NatsHostError::Configuration(
                    "Tracer requires a default Test fixture".into(),
                ));
            }
            if let Some(first) = tracer.dispatch.first() {
                if first.config.context.traffic_scope() != TrafficScope::Normal
                    || first.config.context.application() != self.config.context.application()
                {
                    return Err(NatsHostError::Configuration(
                        "dispatch requires Normal scope in the host application".into(),
                    ));
                }
                group::validate_scope(
                    &first.config,
                    tracer.dispatch.iter().map(|runtime| &runtime.config),
                )?;
            }
            tracer.validate_observation(&self.config)?;
            retained_history::sources(tracer, &self.config)
        } else {
            Ok(Vec::new())
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "keep startup preparation and rollback together"
    )]
    async fn start(self, provision: bool) -> Result<NatsApplication, NatsHostError> {
        let production_stores = self.validate(provision)?;
        let multiple_contexts = !self.contexts.is_empty();
        let dispatch = self
            .tracer
            .as_ref()
            .map_or_else(Vec::new, |tracer| tracer.dispatch.clone());
        let dispatch_group = if !dispatch.is_empty() && (multiple_contexts || dispatch.len() > 1) {
            Some(Arc::new(RuntimeGroup::new(dispatch.clone())?))
        } else {
            None
        };
        if provision {
            provision_application_messaging(self.connection.jetstream(), &self.config.messaging)
                .await?;
        }
        let mut members = Vec::new();
        for (config, bindings) in std::iter::once((self.config, self.bindings)).chain(self.contexts)
        {
            if provision {
                config.provision_context(&self.connection).await?;
            }
            let mut runtime =
                NatsRuntime::connect(self.connection.clone(), config, bindings).await?;
            runtime.grouped_reset = multiple_contexts;
            members.push(Arc::new(runtime));
        }
        let runtime = members.first().cloned().ok_or_else(|| {
            NatsHostError::Configuration("application requires at least one runtime".into())
        })?;
        let test_group = if multiple_contexts {
            Some(Arc::new(TestRuntimeGroup(Arc::new(RuntimeGroup::new(
                members.clone(),
            )?))))
        } else {
            None
        };
        let (stop_tasks, task_shutdown) = watch::channel(false);
        let result = async {
            if let Some(fixture) = &self.initial_fixture {
                if let Some(group) = &test_group {
                    group.apply_fixture(fixture).await?;
                } else {
                    runtime.apply_fixture(fixture).await?;
                }
            }
            let mut tasks = Vec::new();
            let tracer = if let Some(config) = self.tracer {
                let fixture = self.fixture.as_ref().ok_or_else(|| {
                    NatsHostError::Configuration("Tracer requires a default Test fixture".into())
                })?;
                let mut builder = test_group
                    .as_ref()
                    .map_or_else(
                        || {
                            runtime
                                .tracer_builder()
                                .with_test_transport(runtime.transport())
                                .with_test_scenario_reset(runtime.clone())
                        },
                        |group| {
                            group
                                .0
                                .tracer_builder()
                                .with_test_transport(group.0.clone())
                                .with_test_scenario_reset(group.clone())
                        },
                    )
                    .with_test_quarantine_reader(runtime.quarantine_reader())
                    .with_default_test_fixture(fixture.clone())
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
                if let Some(first) = dispatch.first() {
                    let transport = dispatch_group.as_ref().map_or_else(
                        || first.transport(),
                        |group| -> Arc<dyn CommandTransport> { group.clone() },
                    );
                    builder = builder
                        .with_dispatch_transport(transport)
                        .with_production_quarantine_reader(first.quarantine_reader());
                }
                builder = retained_history::attach(
                    builder,
                    &runtime,
                    dispatch.iter().map(Arc::as_ref),
                    production_stores,
                )
                .await?;
                let tracer = builder.build()?;
                for member in &members {
                    *member.observer.lock().await = Some((
                        tracer.correlation_observer(OperationMode::Test),
                        multiple_contexts,
                    ));
                }
                reset_test_scope(&runtime, test_group.as_deref(), fixture).await?;
                group::attach_dispatch_observers(
                    &dispatch,
                    tracer.correlation_observer(OperationMode::Dispatch),
                )
                .await?;
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
                if let Some(fixture) = &self.fixture {
                    reset_test_scope(&runtime, test_group.as_deref(), fixture).await?;
                } else {
                    group::start_fresh(&members).await?;
                }
                None
            };
            for watched in members.iter().chain(&dispatch) {
                let watched = Arc::clone(watched);
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
                runtime, members, dispatch, tracer, tasks, stop_tasks,
            )),
            Err(error) => {
                for member in members.iter().chain(&dispatch) {
                    member.available.store(false, Ordering::Release);
                }
                futures_util::future::join_all(
                    members
                        .iter()
                        .chain(&dispatch)
                        .map(|member| member.stop_workers()),
                )
                .await;
                Err(error)
            }
        }
    }
}

async fn reset_test_scope(
    runtime: &NatsRuntime,
    group: Option<&TestRuntimeGroup>,
    fixture: &Fixture,
) -> Result<(), NatsHostError> {
    if let Some(group) = group {
        group.reset_scope(fixture).await
    } else {
        runtime.reset_scope(fixture).await
    }
}

/// A running application. Call `run_until` to supervise it and always shut down.
/// Dropping it cancels owned HTTP/supervision tasks; `shutdown` awaits cleanup.
pub struct NatsApplication {
    runtime: Arc<NatsRuntime>,
    members: Vec<Arc<NatsRuntime>>,
    dispatch: Vec<Arc<NatsRuntime>>,
    tracer: Option<Tracer>,
    shutdown: watch::Sender<bool>,
    supervisor: Option<Worker>,
}
impl NatsApplication {
    fn supervise(
        runtime: Arc<NatsRuntime>,
        members: Vec<Arc<NatsRuntime>>,
        dispatch: Vec<Arc<NatsRuntime>>,
        tracer: Option<Tracer>,
        tasks: Vec<Worker>,
        stop_tasks: watch::Sender<bool>,
    ) -> Self {
        let (shutdown, mut receiver) = watch::channel(false);
        let watched: Vec<_> = members.iter().chain(&dispatch).cloned().collect();
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
            for runtime in &watched {
                runtime.begin_shutdown();
            }
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
            futures_util::future::join_all(watched.iter().map(|runtime| runtime.stop_workers()))
                .await;
            result
        });
        Self {
            runtime,
            members,
            dispatch,
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
            contexts: Vec::new(),
            fixture: None,
            initial_fixture: None,
            fixtures: Vec::new(),
            tracer: None,
        })
    }
    pub const fn runtime(&self) -> &Arc<NatsRuntime> {
        &self.runtime
    }
    /// Find a context in the primary scope (Test when the host has Tracer).
    pub fn runtime_for(&self, context: &str) -> Option<&Arc<NatsRuntime>> {
        self.members
            .iter()
            .find(|runtime| runtime.config.context.name().as_str() == context)
    }
    pub const fn tracer(&self) -> Option<&Tracer> {
        self.tracer.as_ref()
    }
    fn request_shutdown(&self) {
        for runtime in self.members.iter().chain(&self.dispatch) {
            runtime.begin_shutdown();
        }
        let _ = self.shutdown.send(true);
    }
    pub async fn run_until(
        mut self,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<(), NatsHostError> {
        let Some(mut supervisor) = self.supervisor.take() else {
            return Ok(());
        };
        tokio::select! {
            () = shutdown => { self.request_shutdown(); supervisor.await.map_err(|e| NatsHostError::Worker(e.to_string()))? },
            result = &mut supervisor => result.map_err(|e| NatsHostError::Worker(e.to_string()))?,
        }
    }
    pub async fn shutdown(mut self) -> Result<(), NatsHostError> {
        self.request_shutdown();
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
        self.request_shutdown();
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
