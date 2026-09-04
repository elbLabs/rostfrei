use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, EventHistory, EventStore, EventStoreErrorKind,
    OperationId, SimulationError, StreamDirectory,
};
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, Semaphore};

use crate::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandReceipt, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
    CorrelationCommandOutcome, CorrelationError, CorrelationEventKind, CorrelationObserver,
    CorrelationSubscription, DomainEventObservation, OperationEventKind, OperationMode,
    OperationResult, OperationSnapshot, OperationSubscription, PredictedDomainEvent,
    RuntimeRegistrationError, SubscriptionError,
    behavioral::{
        MAX_EXPOSED_FIXTURE_PAYLOAD_BYTES, MaterializedTestFixture, ResolvedTestDefinition,
        TestCommand, TestDefinitionCollection, TestDefinitionRevision, TestExpectationResult,
        TestFixture, TestOutcome, TestReport, TestReportFailure, TestReportStatus, TestRepository,
        TestRepositoryError, TraceExpectation,
    },
    catalog::{
        AggregateInstanceCollection, AggregateInstanceSummary, TracerCatalog, build_catalog,
    },
    command_execution_fingerprint,
    correlation::CorrelationHub,
    input::{CommandInputDocument, CommandInputOptions},
    operation::{NewOperation, OperationRecord, subscribe},
    runtime::{
        CommandKey, ErasedCommandInputOptions, ErasedCommandSimulator, RuntimeBindings,
        RuntimeDecision, RuntimeSimulationError, stream_id,
    },
    transport::canonical_json_payload,
};

pub const MAX_COMMAND_PAYLOAD_LEN: usize = 1024 * 1024;
const DEFAULT_MAXIMUM_OPERATIONS: usize = 1024;
const DEFAULT_MAXIMUM_CONCURRENT_OPERATIONS: usize = 32;
const DEFAULT_MAXIMUM_OPERATION_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
const MAXIMUM_TOTAL_OPERATION_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationRequest {
    pub schema_version: u32,
    pub payload: Value,
}

#[async_trait]
pub trait TestScenarioReset: Send + Sync {
    /// Recreates isolated Test infrastructure, materializes exactly `fixture`, and starts
    /// workers only after all fixture streams have been written.
    async fn reset(&self, fixture: &MaterializedTestFixture) -> Result<(), TestScenarioResetError>;
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TestScenarioResetError {
    #[error("test scenario reset is not configured")]
    Unavailable,
    #[error("test fixture `{0}` is not registered")]
    UnknownFixture(String),
    #[error("test scenario reset failed: {0}")]
    Failed(String),
}

#[derive(Debug, Error)]
pub enum TestRunError {
    #[error(transparent)]
    Repository(#[from] TestRepositoryError),
    #[error(transparent)]
    Reset(#[from] TestScenarioResetError),
    #[error("test command failed: {0}")]
    CommandFailed(String),
    #[error("test correlation closed before evaluation completed")]
    CorrelationClosed,
    #[error(transparent)]
    Submission(#[from] SubmissionError),
    #[error(transparent)]
    Correlation(#[from] CorrelationError),
}

pub trait TracePayloadPolicy: Send + Sync {
    fn domain_event(&self, event: PredictedDomainEvent) -> PredictedDomainEvent;

    fn rejection(&self, rejection: Value) -> Value;

    fn failure_message(&self, message: String) -> String;

    fn observed_event_payload(&self, _payload: Value) -> Option<Value> {
        None
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RedactTracePayloads;

impl TracePayloadPolicy for RedactTracePayloads {
    fn domain_event(&self, mut event: PredictedDomainEvent) -> PredictedDomainEvent {
        event.payload = None;
        event
    }

    fn rejection(&self, _rejection: Value) -> Value {
        serde_json::json!({ "redacted": true })
    }

    fn failure_message(&self, _message: String) -> String {
        "operation failure details are redacted".to_owned()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExposeTracePayloadsForLocalDevelopment;

impl TracePayloadPolicy for ExposeTracePayloadsForLocalDevelopment {
    fn domain_event(&self, event: PredictedDomainEvent) -> PredictedDomainEvent {
        event
    }

    fn rejection(&self, rejection: Value) -> Value {
        rejection
    }

    fn failure_message(&self, message: String) -> String {
        message
    }

    fn observed_event_payload(&self, payload: Value) -> Option<Value> {
        Some(payload)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SubmissionError {
    #[error(
        "unknown command `{command}` version {schema_version} for aggregate `{aggregate_type}`"
    )]
    UnknownCommand {
        aggregate_type: String,
        command: String,
        schema_version: u32,
    },
    #[error("invalid aggregate identity: {0}")]
    InvalidAggregateId(String),
    #[error("invalid operation identity: {0}")]
    InvalidOperationId(String),
    #[error("command payload exceeds its {maximum}-byte limit")]
    PayloadTooLarge { maximum: usize },
    #[error("operation identity was reused for a different request")]
    IdentityConflict,
    #[error("an idempotency key is required for transported commands")]
    IdempotencyKeyRequired,
    #[error("the test scenario is unavailable until reset succeeds")]
    TestScenarioUnavailable,
    #[error("operation capacity is exhausted")]
    CapacityExhausted,
    #[error("operation concurrency is exhausted")]
    ConcurrencyExhausted,
    #[error("{0} mode is not configured")]
    ModeUnavailable(&'static str),
    #[error("operation was not found")]
    NotFound,
    #[error(transparent)]
    InvalidCursor(#[from] SubscriptionError),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DiscoveryError {
    #[error("aggregate type `{aggregate_type}` is not in the runtime catalog")]
    UnknownAggregate { aggregate_type: String },
    #[error("aggregate instance discovery is not configured")]
    InstanceDiscoveryUnavailable,
    #[error("the test scenario is unavailable until reset succeeds")]
    TestScenarioUnavailable,
    #[error("aggregate instance discovery failed: {0}")]
    Directory(String),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CommandInputError {
    #[error(
        "unknown command `{command}` version {schema_version} for aggregate `{aggregate_type}`"
    )]
    UnknownCommand {
        aggregate_type: String,
        command: String,
        schema_version: u32,
    },
    #[error("invalid aggregate identity: {0}")]
    InvalidAggregateId(String),
    #[error("the test scenario is unavailable until reset succeeds")]
    TestScenarioUnavailable,
    #[error("command input discovery failed: {0}")]
    Runtime(String),
}

pub struct TracerBuilder {
    history: Arc<dyn EventHistory>,
    test_event_store: Option<Arc<dyn EventStore>>,
    test_transport: Option<Arc<dyn CommandTransport>>,
    dispatch_transport: Option<Arc<dyn CommandTransport>>,
    test_scenario_reset: Option<Arc<dyn TestScenarioReset>>,
    test_fixtures: Vec<TestFixture>,
    test_repository: Option<Arc<dyn TestRepository>>,
    bindings: RuntimeBindings,
    domain_model: Option<Value>,
    stream_directory: Option<Arc<dyn StreamDirectory>>,
    maximum_operations: usize,
    maximum_concurrent_operations: usize,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
}

impl TracerBuilder {
    pub fn new(history: Arc<dyn EventHistory>, registry: DomainRegistry) -> Self {
        Self {
            history,
            test_event_store: None,
            test_transport: None,
            dispatch_transport: None,
            test_scenario_reset: None,
            test_fixtures: Vec::new(),
            test_repository: None,
            bindings: RuntimeBindings::new(registry),
            domain_model: None,
            stream_directory: None,
            maximum_operations: DEFAULT_MAXIMUM_OPERATIONS,
            maximum_concurrent_operations: DEFAULT_MAXIMUM_CONCURRENT_OPERATIONS,
            trace_payload_policy: Arc::new(RedactTracePayloads),
        }
    }

    #[must_use]
    pub fn with_domain_model(mut self, domain_model: Value) -> Self {
        self.domain_model = Some(domain_model);
        self
    }

    #[must_use]
    pub fn with_stream_directory(mut self, stream_directory: Arc<dyn StreamDirectory>) -> Self {
        self.stream_directory = Some(stream_directory);
        self
    }

    #[must_use]
    pub fn with_test_event_store<Store>(mut self, store: Arc<Store>) -> Self
    where
        Store: EventStore + 'static,
    {
        self.history = store.clone();
        self.test_event_store = Some(store);
        self
    }

    #[must_use]
    pub fn with_test_transport(mut self, transport: Arc<dyn CommandTransport>) -> Self {
        self.test_transport = Some(transport);
        self
    }

    #[must_use]
    pub fn with_dispatch_transport(mut self, transport: Arc<dyn CommandTransport>) -> Self {
        self.dispatch_transport = Some(transport);
        self
    }

    #[must_use]
    pub fn with_test_scenario_reset(mut self, reset: Arc<dyn TestScenarioReset>) -> Self {
        self.test_scenario_reset = Some(reset);
        self
    }

    #[must_use]
    pub fn with_test_fixture(mut self, fixture: TestFixture) -> Self {
        self.test_fixtures.push(fixture);
        self
    }

    #[must_use]
    pub fn with_test_fixtures(mut self, fixtures: impl IntoIterator<Item = TestFixture>) -> Self {
        self.test_fixtures.extend(fixtures);
        self
    }

    #[must_use]
    pub fn with_test_repository(mut self, repository: Arc<dyn TestRepository>) -> Self {
        self.test_repository = Some(repository);
        self
    }

    #[must_use]
    pub const fn with_maximum_operations(mut self, maximum_operations: usize) -> Self {
        self.maximum_operations = maximum_operations;
        self
    }

    #[must_use]
    pub const fn with_maximum_concurrent_simulations(
        mut self,
        maximum_concurrent_simulations: usize,
    ) -> Self {
        self.maximum_concurrent_operations = maximum_concurrent_simulations;
        self
    }

    #[must_use]
    pub fn with_trace_payload_policy(
        mut self,
        trace_payload_policy: Arc<dyn TracePayloadPolicy>,
    ) -> Self {
        self.trace_payload_policy = trace_payload_policy;
        self
    }

    pub fn register_json<Command>(&mut self) -> Result<&mut Self, RuntimeRegistrationError>
    where
        Command: CommandDefinition + domain::JsonCommandPayload,
        Command::Aggregate: rostfrei_core::CommandHandler<Command>,
        <Command::Aggregate as rostfrei_core::Aggregate>::State: Send,
        <Command::Aggregate as rostfrei_core::Aggregate>::Event: rostfrei_core::Event + Send,
        <Command::Aggregate as rostfrei_core::CommandHandler<Command>>::Rejection:
            domain::JsonErrorPayload,
    {
        self.bindings.register_json::<Command>()?;
        Ok(self)
    }

    pub fn register_input_options<Command, Provider>(
        &mut self,
        provider: Provider,
    ) -> Result<&mut Self, RuntimeRegistrationError>
    where
        Command: CommandDefinition,
        <Command::Aggregate as rostfrei_core::Aggregate>::State: Send,
        <Command::Aggregate as rostfrei_core::Aggregate>::Event: rostfrei_core::Event + Send,
        Provider: CommandInputOptions<Command> + 'static,
    {
        self.bindings
            .register_input_options::<Command, Provider>(provider)?;
        Ok(self)
    }

    pub fn build(self) -> Result<Tracer, RuntimeRegistrationError> {
        self.bindings.validate()?;
        if self.test_scenario_reset.is_some() && self.test_event_store.is_none() {
            return Err(RuntimeRegistrationError::ResetWithoutTestStore);
        }
        if self.test_scenario_reset.is_some() && self.test_transport.is_none() {
            return Err(RuntimeRegistrationError::ResetWithoutTestTransport);
        }
        if self.test_repository.is_some() && self.test_fixtures.is_empty() {
            return Err(RuntimeRegistrationError::TestRepositoryWithoutFixture);
        }
        let mut test_fixtures = BTreeMap::new();
        for fixture in self.test_fixtures {
            fixture
                .validate()
                .map_err(|error| RuntimeRegistrationError::InvalidTestFixture {
                    name: fixture.name.clone(),
                    message: error.to_string(),
                })?;
            for stream in &fixture.streams {
                if !self
                    .bindings
                    .registry
                    .aggregates()
                    .any(|aggregate| aggregate == stream.aggregate_type)
                {
                    return Err(RuntimeRegistrationError::InvalidTestFixture {
                        name: fixture.name.clone(),
                        message: format!(
                            "aggregate type `{}` is not in the domain registry",
                            stream.aggregate_type
                        ),
                    });
                }
            }
            let materialized = fixture.materialize().map_err(|error| {
                RuntimeRegistrationError::InvalidTestFixture {
                    name: fixture.name.clone(),
                    message: error.to_string(),
                }
            })?;
            if test_fixtures
                .insert(materialized.name.clone(), materialized)
                .is_some()
            {
                return Err(RuntimeRegistrationError::DuplicateTestFixture(fixture.name));
            }
        }
        let test_definitions = if let Some(repository) = self.test_repository.as_ref() {
            validate_test_repository(
                repository.as_ref(),
                &test_fixtures,
                &self.bindings.simulators,
            )?
        } else {
            BTreeMap::new()
        };
        let test_enabled = self.test_event_store.is_some() && self.test_transport.is_some();
        let catalog = build_catalog(
            &self.bindings.registry,
            self.domain_model.as_ref(),
            test_enabled,
            self.dispatch_transport.is_some(),
            self.test_scenario_reset.is_some(),
            test_fixtures.keys().cloned().collect(),
            self.test_repository.is_some(),
        );
        let maximum_concurrent_operations = self
            .maximum_concurrent_operations
            .min(self.maximum_operations)
            .min(Semaphore::MAX_PERMITS);
        let maximum_operation_payload_bytes = operation_payload_budget(self.maximum_operations);
        Ok(Tracer {
            inner: Arc::new(TracerInner {
                history: self.history,
                test_backing_configured: self.test_event_store.is_some(),
                test_transport: self.test_transport,
                dispatch_transport: self.dispatch_transport,
                test_scenario_reset: self.test_scenario_reset,
                test_fixtures,
                test_definitions,
                test_scenario_gate: Arc::new(RwLock::new(())),
                test_run_gate: Mutex::new(()),
                simulators: self.bindings.simulators,
                input_options: self.bindings.input_options,
                catalog,
                stream_directory: self.stream_directory,
                operations: Mutex::new(OperationTable::default()),
                correlations: CorrelationHub::new(self.maximum_operations),
                maximum_operations: self.maximum_operations,
                maximum_operation_payload_bytes,
                non_dispatch_permits: Arc::new(Semaphore::new(maximum_concurrent_operations)),
                dispatch_permits: Arc::new(Semaphore::new(maximum_concurrent_operations)),
                generated_ids: AtomicU64::new(0),
                test_generation: AtomicU64::new(0),
                test_run_sequence: AtomicU64::new(0),
                test_scenario_healthy: AtomicBool::new(true),
                trace_payload_policy: self.trace_payload_policy,
            }),
        })
    }
}

fn validate_test_repository(
    repository: &dyn TestRepository,
    fixtures: &BTreeMap<String, MaterializedTestFixture>,
    simulators: &HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
) -> Result<BTreeMap<String, TestDefinitionRevision>, RuntimeRegistrationError> {
    let mut definitions = BTreeMap::new();
    for summary in repository.list().items {
        let revision = repository.get(&summary.id).map_err(|error| {
            RuntimeRegistrationError::InvalidTestDefinition {
                id: summary.id.clone(),
                message: error.to_string(),
            }
        })?;
        let definition = &revision.definition;
        if !fixtures.contains_key(&definition.given.fixture) {
            return Err(RuntimeRegistrationError::UnknownTestFixture {
                test_id: definition.id.clone(),
                fixture: definition.given.fixture.clone(),
            });
        }
        let command = &definition.when.command;
        let key = CommandKey::new(
            &command.aggregate.aggregate_type,
            &command.name,
            command.schema_version,
        );
        let simulator = simulators.get(&key).ok_or_else(|| {
            RuntimeRegistrationError::InvalidTestDefinition {
                id: definition.id.clone(),
                message: format!(
                    "unknown command `{}` version {} for aggregate `{}`",
                    command.name, command.schema_version, command.aggregate.aggregate_type,
                ),
            }
        })?;
        if let Err(message) = simulator.validate_payload(&command.payload) {
            return Err(RuntimeRegistrationError::InvalidTestDefinition {
                id: definition.id.clone(),
                message: format!("invalid payload for command `{}`: {message}", command.name),
            });
        }
        let definition_id = definition.id.clone();
        if definitions
            .insert(definition_id.clone(), revision)
            .is_some()
        {
            return Err(RuntimeRegistrationError::InvalidTestDefinition {
                id: definition_id,
                message: "duplicate test definition".to_owned(),
            });
        }
    }
    Ok(definitions)
}

struct TracerInner {
    history: Arc<dyn EventHistory>,
    test_backing_configured: bool,
    test_transport: Option<Arc<dyn CommandTransport>>,
    dispatch_transport: Option<Arc<dyn CommandTransport>>,
    test_scenario_reset: Option<Arc<dyn TestScenarioReset>>,
    test_fixtures: BTreeMap<String, MaterializedTestFixture>,
    test_definitions: BTreeMap<String, TestDefinitionRevision>,
    test_scenario_gate: Arc<RwLock<()>>,
    test_run_gate: Mutex<()>,
    simulators: HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
    input_options: HashMap<CommandKey, Arc<dyn ErasedCommandInputOptions>>,
    catalog: TracerCatalog,
    stream_directory: Option<Arc<dyn StreamDirectory>>,
    operations: Mutex<OperationTable>,
    correlations: Arc<CorrelationHub>,
    maximum_operations: usize,
    maximum_operation_payload_bytes: usize,
    non_dispatch_permits: Arc<Semaphore>,
    dispatch_permits: Arc<Semaphore>,
    generated_ids: AtomicU64,
    test_generation: AtomicU64,
    test_run_sequence: AtomicU64,
    test_scenario_healthy: AtomicBool,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
}

struct OperationTransportObserver {
    record: Arc<OperationRecord>,
    publication: Mutex<Option<CommandPublication>>,
}

impl OperationTransportObserver {
    fn new(record: Arc<OperationRecord>) -> Self {
        Self {
            record,
            publication: Mutex::new(None),
        }
    }

    async fn matches(&self, receipt: &CommandReceipt) -> bool {
        self.publication
            .lock()
            .await
            .as_ref()
            .is_some_and(|publication| {
                publication.command_message_id() == receipt.command_message_id()
                    && publication.duplicate() == receipt.duplicate()
            })
    }

    async fn publication(&self) -> Option<CommandPublication> {
        self.publication.lock().await.clone()
    }
}

#[async_trait]
impl CommandTransportObserver for OperationTransportObserver {
    async fn command_published(&self, publication: CommandPublication) {
        let mut observed = self.publication.lock().await;
        if observed.is_some() {
            return;
        }
        self.record
            .command_published(
                publication.command_message_id().to_owned(),
                publication.duplicate(),
            )
            .await;
        *observed = Some(publication);
    }
}

#[derive(Default)]
struct OperationTable {
    records: HashMap<String, Arc<OperationRecord>>,
    insertion_order: VecDeque<String>,
}

impl OperationTable {
    fn has_evictable(&self, correlations: &CorrelationHub) -> bool {
        self.records.iter().any(|(operation_id, record)| {
            record.is_evictable() && !correlations.has_active_subscribers(operation_id)
        })
    }

    fn evict_terminal(&mut self, correlations: &CorrelationHub) -> Option<String> {
        for _ in 0..self.insertion_order.len() {
            let operation_id = self.insertion_order.pop_front()?;
            if self
                .records
                .get(&operation_id)
                .is_some_and(|record| record.is_evictable())
                && correlations.remove_if_inactive(&operation_id)
            {
                self.records.remove(&operation_id);
                return Some(operation_id);
            }
            self.insertion_order.push_back(operation_id);
        }
        None
    }

    fn retain_dispatch_operations(&mut self) {
        self.records
            .retain(|_, record| record.mode() == OperationMode::Dispatch);
        self.insertion_order
            .retain(|operation_id| self.records.contains_key(operation_id));
    }
}

#[derive(Clone)]
pub struct Tracer {
    inner: Arc<TracerInner>,
}

struct TestCommandEvaluation {
    operation_id: String,
    correlation_id: String,
    outcome: Option<CorrelationCommandOutcome>,
    expectations: Vec<TestExpectationResult>,
    failure: Option<TestReportFailure>,
}

impl Tracer {
    pub fn catalog(&self) -> &TracerCatalog {
        &self.inner.catalog
    }

    pub fn test_definitions(&self) -> Result<TestDefinitionCollection, TestRepositoryError> {
        if self.inner.test_definitions.is_empty() {
            return Err(TestRepositoryError::Unavailable);
        }
        Ok(TestDefinitionCollection {
            items: self
                .inner
                .test_definitions
                .values()
                .map(TestDefinitionRevision::summary)
                .collect(),
        })
    }

    pub fn test_definition(
        &self,
        test_id: &str,
    ) -> Result<ResolvedTestDefinition, TestRepositoryError> {
        let revision = self
            .inner
            .test_definitions
            .get(test_id)
            .ok_or_else(|| TestRepositoryError::NotFound(test_id.to_owned()))?;
        let fixture = self
            .inner
            .test_fixtures
            .get(&revision.definition.given.fixture)
            .ok_or_else(|| TestRepositoryError::NotFound(test_id.to_owned()))?;
        Ok(ResolvedTestDefinition {
            revision: revision.revision.clone(),
            definition: revision.definition.clone(),
            fixture: self.exposed_fixture(fixture),
        })
    }

    fn exposed_fixture(&self, fixture: &MaterializedTestFixture) -> MaterializedTestFixture {
        let mut fixture = fixture.clone();
        let mut exposed_bytes = 0_usize;
        for event in fixture
            .streams
            .iter_mut()
            .flat_map(|stream| &mut stream.events)
        {
            event.payload = event.payload.take().and_then(|payload| {
                let payload = self
                    .inner
                    .trace_payload_policy
                    .observed_event_payload(payload)?;
                let bytes = serde_json::to_vec(&payload).ok()?.len();
                exposed_bytes = exposed_bytes.checked_add(bytes)?;
                (exposed_bytes <= MAX_EXPOSED_FIXTURE_PAYLOAD_BYTES).then_some(payload)
            });
        }
        fixture
    }

    pub async fn run_test(&self, test_id: &str) -> Result<TestReport, TestRunError> {
        let revision = self
            .inner
            .test_definitions
            .get(test_id)
            .cloned()
            .ok_or_else(|| TestRepositoryError::NotFound(test_id.to_owned()))?;
        let fixture = self
            .inner
            .test_fixtures
            .get(&revision.definition.given.fixture)
            .cloned()
            .ok_or_else(|| TestRepositoryError::NotFound(test_id.to_owned()))?;

        let _test_run = self.inner.test_run_gate.lock().await;
        self.reset_test_scenario_unlocked(&fixture).await?;
        let sequence = self.inner.test_run_sequence.fetch_add(1, Ordering::Relaxed);
        let run_id = format!("test-run-{sequence}");
        let evaluation = self
            .evaluate_test_command(
                &revision.definition.when.command,
                &revision.definition.then.outcome,
                &revision.definition.then.trace.contains,
                revision.definition.then.within.as_duration(),
                &format!("{run_id}-subject"),
            )
            .await?;
        let status = if evaluation.failure.is_some() {
            TestReportStatus::Failed
        } else {
            TestReportStatus::Passed
        };
        Ok(TestReport {
            run_id,
            test_id: revision.definition.id,
            revision: revision.revision,
            fixture: self.exposed_fixture(&fixture),
            status,
            operation_id: evaluation.operation_id,
            correlation_id: evaluation.correlation_id,
            outcome: evaluation.outcome,
            expectations: evaluation.expectations,
            failure: evaluation.failure,
        })
    }

    #[allow(clippy::too_many_lines)]
    async fn evaluate_test_command(
        &self,
        command: &TestCommand,
        expected_outcome: &TestOutcome,
        expected_trace: &[TraceExpectation],
        timeout: Duration,
        idempotency_key: &str,
    ) -> Result<TestCommandEvaluation, TestRunError> {
        let queued = self
            .submit_test_unlocked(
                &command.aggregate.aggregate_type,
                &command.aggregate.id,
                &command.name,
                SimulationRequest {
                    schema_version: command.schema_version,
                    payload: command.payload.clone(),
                },
                Some(idempotency_key),
            )
            .await?;
        let mut subscription = self
            .subscribe_correlation(&queued.correlation_id, 0)
            .await?;
        let mut expectations = expected_trace
            .iter()
            .cloned()
            .map(|expectation| TestExpectationResult {
                expectation,
                matched_event_id: None,
            })
            .collect::<Vec<_>>();
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let mut outcome = None;

        loop {
            tokio::select! {
                event = subscription.next() => {
                    let Some(event) = event else {
                        return Err(TestRunError::CorrelationClosed);
                    };
                    match &event.kind {
                        CorrelationEventKind::DomainEvent { .. }
                        | CorrelationEventKind::IntegrationEvent { .. } => {
                            if let Some(expectation) = expectations.iter_mut().find(|expectation| {
                                expectation.matched_event_id.is_none()
                                    && expectation.expectation.matches(&event)
                            }) {
                                expectation.matched_event_id = Some(event.id);
                            }
                        }
                        CorrelationEventKind::CommandResult {
                            outcome: actual,
                            result,
                            ..
                        } => {
                            outcome = Some(*actual);
                            match actual {
                                CorrelationCommandOutcome::Failed
                                | CorrelationCommandOutcome::Indeterminate => {
                                    return Err(TestRunError::CommandFailed(
                                        result.as_ref().map_or_else(
                                            || "command execution failed without details".to_owned(),
                                            Value::to_string,
                                        ),
                                    ));
                                }
                                CorrelationCommandOutcome::Accepted
                                | CorrelationCommandOutcome::Rejected => {
                                    if !expected_outcome.matches(*actual, result.as_ref()) {
                                        return Ok(TestCommandEvaluation {
                                            operation_id: queued.operation_id,
                                            correlation_id: queued.correlation_id,
                                            outcome,
                                            expectations,
                                            failure: Some(TestReportFailure {
                                                code: "unexpected-outcome",
                                                message: format!(
                                                    "command outcome `{actual:?}` did not match the test definition"
                                                ),
                                            }),
                                        });
                                    }
                                }
                            }
                        }
                        CorrelationEventKind::Command { .. } => {}
                    }

                    if outcome.is_some()
                        && expectations
                            .iter()
                            .all(|expectation| expectation.matched_event_id.is_some())
                    {
                        return Ok(TestCommandEvaluation {
                            operation_id: queued.operation_id,
                            correlation_id: queued.correlation_id,
                            outcome,
                            expectations,
                            failure: None,
                        });
                    }
                }
                () = &mut deadline => {
                    self.abort_operation(&queued.operation_id).await;
                    return Ok(TestCommandEvaluation {
                        operation_id: queued.operation_id,
                        correlation_id: queued.correlation_id,
                        outcome,
                        expectations,
                        failure: Some(TestReportFailure {
                            code: "deadline-exceeded",
                            message: "the expected correlated behavior was not observed before the deadline".to_owned(),
                        }),
                    });
                }
            }
        }
    }

    pub async fn aggregate_instances(
        &self,
        aggregate_type: &str,
    ) -> Result<AggregateInstanceCollection, DiscoveryError> {
        let _scenario = Arc::clone(&self.inner.test_scenario_gate)
            .read_owned()
            .await;
        if !self.inner.test_scenario_healthy.load(Ordering::Acquire) {
            return Err(DiscoveryError::TestScenarioUnavailable);
        }
        let known = self.inner.catalog.contexts.iter().any(|context| {
            context
                .aggregates
                .iter()
                .any(|aggregate| aggregate.aggregate_type == aggregate_type)
        });
        if !known {
            return Err(DiscoveryError::UnknownAggregate {
                aggregate_type: aggregate_type.to_owned(),
            });
        }
        let directory = self
            .inner
            .stream_directory
            .as_ref()
            .ok_or(DiscoveryError::InstanceDiscoveryUnavailable)?;
        let aggregate_type = AggregateType::new(aggregate_type)
            .map_err(|error| DiscoveryError::Directory(error.to_string()))?;
        let streams = directory
            .list_streams(&aggregate_type)
            .await
            .map_err(|error| DiscoveryError::Directory(error.to_string()))?;
        Ok(AggregateInstanceCollection {
            items: streams
                .into_iter()
                .map(|stream| AggregateInstanceSummary {
                    aggregate_id: stream.stream_id().aggregate_id().as_str().to_owned(),
                    stream_version: stream.stream_version().value(),
                })
                .collect(),
        })
    }

    pub async fn command_inputs(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        schema_version: u32,
    ) -> Result<CommandInputDocument, CommandInputError> {
        let _scenario = Arc::clone(&self.inner.test_scenario_gate)
            .read_owned()
            .await;
        if !self.inner.test_scenario_healthy.load(Ordering::Acquire) {
            return Err(CommandInputError::TestScenarioUnavailable);
        }
        let key = CommandKey::new(aggregate_type, command, schema_version);
        let simulator =
            self.inner
                .simulators
                .get(&key)
                .ok_or_else(|| CommandInputError::UnknownCommand {
                    aggregate_type: aggregate_type.to_owned(),
                    command: command.to_owned(),
                    schema_version,
                })?;
        let Some(provider) = self.inner.input_options.get(&key) else {
            return Ok(CommandInputDocument { fields: Vec::new() });
        };
        let aggregate_id = AggregateId::new(aggregate_id)
            .map_err(|error| CommandInputError::InvalidAggregateId(error.to_string()))?;
        let stream = stream_id(simulator.descriptor(), aggregate_id)
            .map_err(|error| CommandInputError::InvalidAggregateId(error.to_string()))?;
        provider
            .fields(Arc::clone(&self.inner.history), stream)
            .await
            .map_err(|error| CommandInputError::Runtime(error.to_string()))
    }

    pub async fn reset_test_scenario(
        &self,
        fixture_name: &str,
    ) -> Result<(), TestScenarioResetError> {
        let fixture = self
            .inner
            .test_fixtures
            .get(fixture_name)
            .cloned()
            .ok_or_else(|| TestScenarioResetError::UnknownFixture(fixture_name.to_owned()))?;
        let _test_run = self.inner.test_run_gate.lock().await;
        self.reset_test_scenario_unlocked(&fixture).await
    }

    async fn reset_test_scenario_unlocked(
        &self,
        fixture: &MaterializedTestFixture,
    ) -> Result<(), TestScenarioResetError> {
        let reset = self
            .inner
            .test_scenario_reset
            .as_ref()
            .ok_or(TestScenarioResetError::Unavailable)?;
        let _scenario = Arc::clone(&self.inner.test_scenario_gate)
            .write_owned()
            .await;
        self.inner
            .test_scenario_healthy
            .store(false, Ordering::Release);
        self.inner.test_generation.fetch_add(1, Ordering::AcqRel);
        self.inner
            .operations
            .lock()
            .await
            .retain_dispatch_operations();
        self.inner.correlations.retain_dispatch_correlations();
        let result = reset.reset(fixture).await;
        if result.is_ok() {
            self.inner
                .test_scenario_healthy
                .store(true, Ordering::Release);
        }
        result
    }

    pub async fn submit_simulation(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OperationSnapshot, SubmissionError> {
        self.submit_operation(
            OperationMode::Simulate,
            aggregate_type,
            aggregate_id,
            command,
            request,
            idempotency_key,
        )
        .await
    }

    pub async fn submit_test(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OperationSnapshot, SubmissionError> {
        let _test_run = self.inner.test_run_gate.lock().await;
        self.submit_test_unlocked(
            aggregate_type,
            aggregate_id,
            command,
            request,
            idempotency_key,
        )
        .await
    }

    async fn submit_test_unlocked(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OperationSnapshot, SubmissionError> {
        self.submit_operation(
            OperationMode::Test,
            aggregate_type,
            aggregate_id,
            command,
            request,
            idempotency_key,
        )
        .await
    }

    pub async fn submit_dispatch(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OperationSnapshot, SubmissionError> {
        self.submit_operation(
            OperationMode::Dispatch,
            aggregate_type,
            aggregate_id,
            command,
            request,
            idempotency_key,
        )
        .await
    }

    #[allow(clippy::too_many_lines)]
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the reset guard must remain held until the spawned operation completes"
    )]
    async fn submit_operation(
        &self,
        mode: OperationMode,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OperationSnapshot, SubmissionError> {
        let transport = match mode {
            OperationMode::Simulate => None,
            OperationMode::Test => {
                if !self.inner.test_backing_configured {
                    return Err(SubmissionError::ModeUnavailable("test"));
                }
                Some(
                    self.inner
                        .test_transport
                        .clone()
                        .ok_or(SubmissionError::ModeUnavailable("test"))?,
                )
            }
            OperationMode::Dispatch => Some(
                self.inner
                    .dispatch_transport
                    .clone()
                    .ok_or(SubmissionError::ModeUnavailable("dispatch"))?,
            ),
        };
        if mode != OperationMode::Simulate && idempotency_key.is_none() {
            return Err(SubmissionError::IdempotencyKeyRequired);
        }
        let key = CommandKey::new(aggregate_type, command, request.schema_version);
        let simulator = self.inner.simulators.get(&key).cloned().ok_or_else(|| {
            SubmissionError::UnknownCommand {
                aggregate_type: aggregate_type.to_owned(),
                command: command.to_owned(),
                schema_version: request.schema_version,
            }
        })?;
        let aggregate_id = AggregateId::new(aggregate_id)
            .map_err(|error| SubmissionError::InvalidAggregateId(error.to_string()))?;
        #[allow(
            clippy::significant_drop_tightening,
            reason = "the spawned operation must retain the scenario guard until completion"
        )]
        let scenario_guard = if mode == OperationMode::Dispatch {
            None
        } else {
            Some(
                Arc::clone(&self.inner.test_scenario_gate)
                    .read_owned()
                    .await,
            )
        };
        if scenario_guard.is_some() && !self.inner.test_scenario_healthy.load(Ordering::Acquire) {
            return Err(SubmissionError::TestScenarioUnavailable);
        }
        let operation_id = match idempotency_key {
            Some(value) => {
                let value = validate_http_operation_id(value)?;
                if mode == OperationMode::Simulate {
                    if value.starts_with("test:") || value.starts_with("dispatch:") {
                        return Err(SubmissionError::InvalidOperationId(
                            "simulation idempotency keys cannot use a transported-operation namespace"
                                .to_owned(),
                        ));
                    }
                    OperationId::new(value)
                        .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))?
                } else {
                    operation_id_from_key(
                        mode,
                        if mode == OperationMode::Test {
                            self.inner.test_generation.load(Ordering::Acquire)
                        } else {
                            0
                        },
                        aggregate_type,
                        aggregate_id.as_str(),
                        command,
                        value,
                    )?
                }
            }
            None => self.generated_operation_id(mode)?,
        };
        let request_bytes = canonical_json_payload(&request.payload);
        let maximum_payload_len = transport
            .as_ref()
            .map_or(MAX_COMMAND_PAYLOAD_LEN, |transport| {
                transport.maximum_payload_len().min(MAX_COMMAND_PAYLOAD_LEN)
            });
        if request_bytes.len() > maximum_payload_len {
            return Err(SubmissionError::PayloadTooLarge {
                maximum: maximum_payload_len,
            });
        }
        let operation_fingerprint = request_fingerprint(
            mode,
            aggregate_type,
            aggregate_id.as_str(),
            command,
            request.schema_version,
            &request_bytes,
        );
        let execution_fingerprint = command_execution_fingerprint(
            aggregate_type,
            aggregate_id.as_str(),
            command,
            request.schema_version,
            &request.payload,
        );
        let operation_key = operation_id.as_str().to_owned();
        let correlation_id = operation_key.clone();
        let record = OperationRecord::new(NewOperation {
            operation_id: operation_key.clone(),
            correlation_id: correlation_id.clone(),
            fingerprint: operation_fingerprint.to_hex(),
            mode,
            command,
            schema_version: request.schema_version,
            aggregate_type,
            aggregate_id: aggregate_id.as_str(),
        });
        let queued = record.snapshot().await;

        let permit = {
            let mut operations = self.inner.operations.lock().await;
            if let Some(existing) = operations.records.get(&operation_key) {
                if existing.fingerprint().await != operation_fingerprint.to_hex() {
                    drop(operations);
                    return Err(SubmissionError::IdentityConflict);
                }
                let snapshot = existing.snapshot().await;
                drop(operations);
                return Ok(snapshot);
            }
            if operations.records.len() >= self.inner.maximum_operations
                && !operations.has_evictable(&self.inner.correlations)
            {
                drop(operations);
                return Err(SubmissionError::CapacityExhausted);
            }
            let permits = if mode == OperationMode::Dispatch {
                &self.inner.dispatch_permits
            } else {
                &self.inner.non_dispatch_permits
            };
            let permit = Arc::clone(permits)
                .try_acquire_owned()
                .map_err(|_| SubmissionError::ConcurrencyExhausted)?;
            if operations.records.len() >= self.inner.maximum_operations
                && operations
                    .evict_terminal(&self.inner.correlations)
                    .is_none()
            {
                drop(operations);
                return Err(SubmissionError::CapacityExhausted);
            }
            operations.insertion_order.push_back(operation_key.clone());
            operations
                .records
                .insert(operation_key.clone(), Arc::clone(&record));
            if let Err(error) = self.inner.correlations.register_command(
                &correlation_id,
                mode,
                operation_id.as_str().to_owned(),
                command.to_owned(),
                request.schema_version,
                aggregate_type.to_owned(),
                aggregate_id.as_str().to_owned(),
            ) {
                operations.records.remove(&operation_key);
                operations
                    .insertion_order
                    .retain(|retained| retained != &operation_key);
                let error = match error {
                    CorrelationError::CapacityExhausted => SubmissionError::CapacityExhausted,
                    _ => SubmissionError::InvalidOperationId(error.to_string()),
                };
                drop(operations);
                return Err(error);
            }
            drop(operations);
            permit
        };

        let tracer = self.clone();
        let panic_tracer = self.clone();
        let panic_record = Arc::clone(&record);
        let result_record = Arc::clone(&record);
        let result_correlation_id = correlation_id.clone();
        let panic_correlation_id = correlation_id;
        let execution_operation_id = operation_id;
        let aggregate_type = aggregate_type.to_owned();
        let command = command.to_owned();
        let execution = tokio::spawn(async move {
            let _permit = permit;
            let _scenario_guard = scenario_guard;
            tracer
                .run_operation(
                    record,
                    simulator,
                    mode,
                    transport,
                    aggregate_type,
                    aggregate_id,
                    command,
                    request.schema_version,
                    execution_operation_id,
                    result_correlation_id.clone(),
                    execution_fingerprint,
                    request.payload,
                )
                .await;
            tracer
                .record_correlation_result(&result_correlation_id, &result_record)
                .await;
        });
        panic_record.set_execution(execution.abort_handle());
        tokio::spawn(async move {
            if execution.await.is_err() {
                panic_record
                    .fail_after_possible_publication(
                        "operation-panicked",
                        "the command operation task panicked".to_owned(),
                    )
                    .await;
                panic_tracer
                    .record_correlation_result(&panic_correlation_id, &panic_record)
                    .await;
            }
        });
        Ok(queued)
    }

    pub async fn operation(
        &self,
        operation_id: &str,
    ) -> Result<OperationSnapshot, SubmissionError> {
        let record = self.record(operation_id).await?;
        Ok(record.snapshot().await)
    }

    async fn abort_operation(&self, operation_id: &str) {
        let record = self
            .inner
            .operations
            .lock()
            .await
            .records
            .get(operation_id)
            .cloned();
        if let Some(record) = record {
            record.abort_and_wait().await;
        }
    }

    pub async fn subscribe(
        &self,
        operation_id: &str,
        after: u64,
    ) -> Result<OperationSubscription, SubmissionError> {
        let record = self.record(operation_id).await?;
        Ok(subscribe(&record, after).await?)
    }

    pub async fn subscribe_with_snapshot(
        &self,
        operation_id: &str,
        after: u64,
    ) -> Result<(OperationSnapshot, OperationSubscription), SubmissionError> {
        let record = self.record(operation_id).await?;
        let snapshot = record.snapshot().await;
        let subscription = subscribe(&record, after).await?;
        Ok((snapshot, subscription))
    }

    pub fn correlation_observer(&self, mode: OperationMode) -> CorrelationObserver {
        self.inner
            .correlations
            .observer(mode, Arc::clone(&self.inner.trace_payload_policy))
    }

    pub fn correlation_mode(
        &self,
        correlation_id: &str,
    ) -> Result<OperationMode, CorrelationError> {
        self.inner.correlations.mode(correlation_id)
    }

    pub async fn subscribe_correlation(
        &self,
        correlation_id: &str,
        after: u64,
    ) -> Result<CorrelationSubscription, CorrelationError> {
        self.inner
            .correlations
            .subscribe(correlation_id, after)
            .await
    }

    pub async fn subscribe_correlation_with_mode(
        &self,
        correlation_id: &str,
        after: u64,
    ) -> Result<(OperationMode, CorrelationSubscription), CorrelationError> {
        self.inner
            .correlations
            .subscribe_with_mode(correlation_id, after)
            .await
    }

    async fn record(&self, operation_id: &str) -> Result<Arc<OperationRecord>, SubmissionError> {
        OperationId::new(operation_id)
            .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))?;
        self.inner
            .operations
            .lock()
            .await
            .records
            .get(operation_id)
            .cloned()
            .ok_or(SubmissionError::NotFound)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn run_operation(
        &self,
        record: Arc<OperationRecord>,
        simulator: Arc<dyn ErasedCommandSimulator>,
        mode: OperationMode,
        transport: Option<Arc<dyn CommandTransport>>,
        aggregate_type: String,
        aggregate_id: AggregateId,
        command: String,
        schema_version: u32,
        operation_id: OperationId,
        correlation_id: String,
        execution_fingerprint: ContentFingerprint,
        payload: Value,
    ) {
        record.start().await;
        if let Some(transport) = transport {
            if let Err(error) = simulator.validate_payload(&payload) {
                record
                    .fail(
                        "invalid-command-payload",
                        bounded_failure_message(
                            self.inner.trace_payload_policy.as_ref(),
                            error,
                            self.inner.maximum_operation_payload_bytes,
                        ),
                    )
                    .await;
                return;
            }
            let invocation = CommandInvocation::new(
                operation_id,
                correlation_id,
                execution_fingerprint,
                aggregate_type,
                aggregate_id,
                command,
                schema_version,
                payload,
            );
            let observer = Arc::new(OperationTransportObserver::new(Arc::clone(&record)));
            match transport.invoke(invocation, observer.clone()).await {
                Ok(receipt) if observer.matches(&receipt).await => {
                    complete_transport(
                        &record,
                        receipt,
                        self.inner.trace_payload_policy.as_ref(),
                        self.inner.maximum_operation_payload_bytes,
                    )
                    .await;
                }
                Ok(_) => {
                    let message = bounded_failure_message(
                        self.inner.trace_payload_policy.as_ref(),
                        "command transport returned a receipt without a matching publication observation"
                            .to_owned(),
                        self.inner.maximum_operation_payload_bytes,
                    );
                    if let Some(publication) = observer.publication().await {
                        record
                            .indeterminate(
                                "invalid-command-transport-receipt",
                                message,
                                publication.command_message_id().to_owned(),
                                publication.duplicate(),
                            )
                            .await;
                    } else {
                        record
                            .fail("invalid-command-transport-receipt", message)
                            .await;
                    }
                }
                Err(error) => {
                    let (code, message) = transport_failure(&error);
                    let message = bounded_failure_message(
                        self.inner.trace_payload_policy.as_ref(),
                        message,
                        self.inner.maximum_operation_payload_bytes,
                    );
                    if let Some(publication) = observer.publication().await {
                        record
                            .indeterminate(
                                code,
                                message,
                                publication.command_message_id().to_owned(),
                                publication.duplicate(),
                            )
                            .await;
                    } else {
                        record.fail(code, message).await;
                    }
                }
            }
            return;
        }

        let stream = match stream_id(simulator.descriptor(), aggregate_id) {
            Ok(stream) => stream,
            Err(error) => {
                record
                    .fail(
                        "invalid-runtime",
                        bounded_failure_message(
                            self.inner.trace_payload_policy.as_ref(),
                            error.to_string(),
                            self.inner.maximum_operation_payload_bytes,
                        ),
                    )
                    .await;
                return;
            }
        };
        match simulator
            .simulate(
                Arc::clone(&self.inner.history),
                stream,
                operation_id,
                execution_fingerprint,
                payload,
            )
            .await
        {
            Ok(RuntimeDecision::Accepted {
                base_stream_version,
                events,
            }) => {
                let mut events: Vec<PredictedDomainEvent> = events
                    .into_iter()
                    .map(|event| self.inner.trace_payload_policy.domain_event(event))
                    .collect();
                bound_predicted_event_payloads(
                    &mut events,
                    self.inner.maximum_operation_payload_bytes,
                );
                for event in &events {
                    let mut observation =
                        DomainEventObservation::new(event.event_type.clone(), event.schema_version)
                            .with_stream_version(event.predicted_stream_version);
                    if let Some(payload) = event.payload.clone() {
                        observation = observation.with_payload(payload);
                    }
                    let _ = self
                        .inner
                        .correlations
                        .observer(mode, Arc::clone(&self.inner.trace_payload_policy))
                        .observe_domain_event(&correlation_id, observation)
                        .await;
                }
                complete_simulation_accepted(&record, base_stream_version, events).await;
            }
            Ok(RuntimeDecision::Rejected {
                base_stream_version,
                rejection,
            }) => {
                let rejection = bounded_rejection(
                    self.inner.trace_payload_policy.rejection(rejection),
                    self.inner.maximum_operation_payload_bytes,
                );
                complete_simulation_rejected(&record, base_stream_version, rejection).await;
            }
            Err(error) => {
                let (code, message) = runtime_failure(error);
                record
                    .fail(
                        code,
                        bounded_failure_message(
                            self.inner.trace_payload_policy.as_ref(),
                            message,
                            self.inner.maximum_operation_payload_bytes,
                        ),
                    )
                    .await;
            }
        }
    }

    async fn record_correlation_result(&self, correlation_id: &str, record: &OperationRecord) {
        let snapshot = record.snapshot().await;
        let (outcome, result) = if let Some(result) = snapshot.result {
            let outcome = match result {
                OperationResult::Accepted { .. } => CorrelationCommandOutcome::Accepted,
                OperationResult::Rejected { .. } => CorrelationCommandOutcome::Rejected,
            };
            (outcome, serde_json::to_value(result).ok())
        } else if snapshot.status == crate::OperationStatus::Indeterminate {
            (
                CorrelationCommandOutcome::Indeterminate,
                snapshot
                    .failure
                    .and_then(|failure| serde_json::to_value(failure).ok()),
            )
        } else {
            (
                CorrelationCommandOutcome::Failed,
                snapshot
                    .failure
                    .and_then(|failure| serde_json::to_value(failure).ok()),
            )
        };
        let _ = self
            .inner
            .correlations
            .command_result(correlation_id, snapshot.operation_id, outcome, result)
            .await;
        record.mark_correlation_recorded();
    }

    fn generated_operation_id(&self, mode: OperationMode) -> Result<OperationId, SubmissionError> {
        let sequence = self.inner.generated_ids.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let prefix = match mode {
            OperationMode::Simulate => "simulation",
            OperationMode::Test => "test",
            OperationMode::Dispatch => "dispatch",
        };
        OperationId::new(format!("{prefix}-{nanos:x}-{sequence:x}"))
            .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))
    }
}

fn validate_http_operation_id(value: &str) -> Result<&str, SubmissionError> {
    OperationId::new(value)
        .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(SubmissionError::InvalidOperationId(
            "idempotency key must use only ASCII letters, digits, '-', '_', '.', or ':'".to_owned(),
        ));
    }
    Ok(value)
}

fn operation_id_from_key(
    mode: OperationMode,
    generation: u64,
    aggregate_type: &str,
    aggregate_id: &str,
    command: &str,
    idempotency_key: &str,
) -> Result<OperationId, SubmissionError> {
    let digest = framed_fingerprint(&[
        b"rostfrei:tracer-operation:v1".as_slice(),
        mode.as_str().as_bytes(),
        generation.to_be_bytes().as_slice(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
        command.as_bytes(),
        idempotency_key.as_bytes(),
    ]);
    OperationId::new(format!("{}:{}", mode.as_str(), digest.to_hex()))
        .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))
}

async fn complete_simulation_accepted(
    record: &OperationRecord,
    base_stream_version: u64,
    events: Vec<PredictedDomainEvent>,
) {
    let mut trace = vec![
        OperationEventKind::HistoryReplayed {
            base_stream_version,
        },
        OperationEventKind::CommandAccepted,
    ];
    trace.extend(
        events
            .iter()
            .cloned()
            .map(|event| OperationEventKind::PredictedDomainEvent { event }),
    );
    record
        .complete(
            OperationResult::Accepted {
                base_stream_version: Some(base_stream_version),
                predicted_events: events,
                appended: Some(false),
                published: false,
                command_message_id: None,
                response_message_id: None,
                duplicate: None,
            },
            trace,
        )
        .await;
}

async fn complete_simulation_rejected(
    record: &OperationRecord,
    base_stream_version: u64,
    rejection: Value,
) {
    record
        .complete(
            OperationResult::Rejected {
                base_stream_version: Some(base_stream_version),
                rejection: rejection.clone(),
                appended: Some(false),
                published: false,
                command_message_id: None,
                response_message_id: None,
                duplicate: None,
            },
            vec![
                OperationEventKind::HistoryReplayed {
                    base_stream_version,
                },
                OperationEventKind::CommandRejected { rejection },
            ],
        )
        .await;
}

async fn complete_transport(
    record: &OperationRecord,
    receipt: CommandReceipt,
    trace_payload_policy: &dyn TracePayloadPolicy,
    maximum_payload_bytes: usize,
) {
    let (command_message_id, response_message_id, duplicate, outcome) = receipt.into_parts();
    let responded = OperationEventKind::CommandResponded {
        response_message_id: response_message_id.clone(),
    };
    match outcome {
        CommandOutcome::Accepted => {
            record
                .complete(
                    OperationResult::Accepted {
                        base_stream_version: None,
                        predicted_events: Vec::new(),
                        appended: None,
                        published: true,
                        command_message_id: Some(command_message_id),
                        response_message_id: Some(response_message_id),
                        duplicate: Some(duplicate),
                    },
                    vec![responded, OperationEventKind::CommandAccepted],
                )
                .await;
        }
        CommandOutcome::Rejected(rejection) => {
            let rejection = bounded_rejection(
                trace_payload_policy.rejection(rejection.into_value()),
                maximum_payload_bytes,
            );
            record
                .complete(
                    OperationResult::Rejected {
                        base_stream_version: None,
                        rejection: rejection.clone(),
                        appended: None,
                        published: true,
                        command_message_id: Some(command_message_id),
                        response_message_id: Some(response_message_id),
                        duplicate: Some(duplicate),
                    },
                    vec![responded, OperationEventKind::CommandRejected { rejection }],
                )
                .await;
        }
    }
}

fn operation_payload_budget(maximum_operations: usize) -> usize {
    MAXIMUM_TOTAL_OPERATION_PAYLOAD_BYTES
        .checked_div(maximum_operations.max(1))
        .unwrap_or(MAXIMUM_TOTAL_OPERATION_PAYLOAD_BYTES)
        .min(DEFAULT_MAXIMUM_OPERATION_PAYLOAD_BYTES)
}

fn bound_predicted_event_payloads(events: &mut [PredictedDomainEvent], maximum_bytes: usize) {
    // The operation result and event journal each retain a copy of every exposed payload.
    let mut remaining = maximum_bytes / 2;
    for event in events {
        let Some(payload) = event.payload.as_ref() else {
            continue;
        };
        let bytes = serialized_value_len(payload);
        if bytes > remaining {
            event.payload = None;
        } else {
            remaining = remaining.saturating_sub(bytes);
        }
    }
}

fn bounded_rejection(rejection: Value, maximum_bytes: usize) -> Value {
    if serialized_value_len(&rejection) <= maximum_bytes / 2 {
        rejection
    } else {
        serde_json::json!({ "omitted": true })
    }
}

fn bounded_failure_message(
    trace_payload_policy: &dyn TracePayloadPolicy,
    message: String,
    maximum_bytes: usize,
) -> String {
    let mut message = trace_payload_policy.failure_message(message);
    let maximum_bytes = maximum_bytes / 2;
    if message.len() > maximum_bytes {
        let mut end = maximum_bytes;
        while !message.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        message.truncate(end);
    }
    message
}

fn serialized_value_len(value: &Value) -> usize {
    value.to_string().len()
}

fn transport_failure(error: &CommandTransportError) -> (&'static str, String) {
    let code = match error.kind() {
        CommandTransportErrorKind::InvalidRequest => "invalid-command-transport-request",
        CommandTransportErrorKind::Rejected => "command-transport-rejected",
        CommandTransportErrorKind::Timeout => "command-transport-timeout",
        CommandTransportErrorKind::Unavailable => "command-transport-unavailable",
        CommandTransportErrorKind::InvalidConfiguration => "command-transport-misconfigured",
        CommandTransportErrorKind::InvalidResponse => "invalid-command-transport-response",
    };
    (code, error.message().to_owned())
}

fn runtime_failure(error: RuntimeSimulationError) -> (&'static str, String) {
    match error {
        RuntimeSimulationError::InvalidPayload(error) => ("invalid-command-payload", error),
        RuntimeSimulationError::Simulation(SimulationError::Codec(error)) => {
            ("event-json-failed", error.to_string())
        }
        RuntimeSimulationError::Simulation(SimulationError::Store(error)) => {
            let code = match error.kind() {
                EventStoreErrorKind::CorruptHistory => "corrupt-history",
                EventStoreErrorKind::Unavailable
                | EventStoreErrorKind::CapacityExhausted
                | EventStoreErrorKind::ConfigurationMismatch => "history-unavailable",
                EventStoreErrorKind::Conflict | EventStoreErrorKind::IdentityConflict => {
                    "history-conflict"
                }
                EventStoreErrorKind::InvalidRequest => "invalid-runtime",
            };
            (code, error.to_string())
        }
        RuntimeSimulationError::RejectionEncoding(error) => ("rejection-encoding-failed", error),
        RuntimeSimulationError::InvalidEventPayload(error) => ("event-json-failed", error),
        RuntimeSimulationError::StreamVersionOverflow => {
            ("stream-version-overflow", error.to_string())
        }
    }
}

fn request_fingerprint(
    mode: OperationMode,
    aggregate_type: &str,
    aggregate_id: &str,
    command: &str,
    schema_version: u32,
    payload: &[u8],
) -> ContentFingerprint {
    let schema_version = schema_version.to_be_bytes();
    framed_fingerprint(&[
        b"rostfrei:tracer-request:v2".as_slice(),
        mode.as_str().as_bytes(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
        command.as_bytes(),
        schema_version.as_slice(),
        payload,
    ])
}

fn framed_fingerprint(values: &[&[u8]]) -> ContentFingerprint {
    let mut framed = Vec::new();
    for value in values {
        let length = u64::try_from(value.len()).unwrap_or(u64::MAX);
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(value);
    }
    ContentFingerprint::digest(framed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_fingerprints_use_deterministic_fixed_width_framing() {
        let fingerprint = request_fingerprint(
            OperationMode::Simulate,
            "bike-rental/rental-fleet",
            "city-fleet",
            "rent-bicycle",
            1,
            br#"{"bicycle_id":"bike-42"}"#,
        );

        assert_eq!(
            fingerprint.to_hex(),
            "9300fe8edfdb87c65efd101d49fb3eefeedde2020109cea3b2628464f1af35af"
        );
    }
}
