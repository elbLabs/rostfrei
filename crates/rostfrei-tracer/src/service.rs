use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, CommandHandler, ContentFingerprint, Event, EventHistory,
    EventStore, EventStoreErrorKind, OperationId, SimulationError, StreamDirectory,
};
use rostfrei_messaging_core::{
    ApplicationErrorCode, CommandRejection as MessagingCommandRejection,
    CommandRejectionClassification, CommandResponseOutcome,
};
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, Semaphore};

use crate::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandReceipt, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
    CorrelationCommandOutcome, CorrelationError, CorrelationObserver, CorrelationSubscription,
    DomainEventObservation, OperationEventKind, OperationMode, OperationResult, OperationSnapshot,
    OperationSubscription, PredictedDomainEvent, RuntimeRegistrationError, SubscriptionError,
    behavioral::{
        TestAggregate, TestCommand, TestDefinition, TestDefinitionCollection,
        TestDefinitionRevision, TestReport, TestReportStatus, TestRepository, TestRepositoryError,
    },
    catalog::{
        AggregateInstanceCollection, AggregateInstanceSummary, TracerCatalog, build_catalog,
    },
    command_execution_fingerprint,
    correlation::{CorrelationEvidenceSnapshot, CorrelationHub},
    input::{CommandInputDocument, CommandInputOptions},
    message_series::{
        MessageGraphDefinition, MessageSeriesComparison, MessageSeriesComparisonContext,
        MessageSeriesComparisonDiagnostic, MessageSeriesComparisonStatus, MessageSeriesDefinition,
        ObservedCommandOutcome, ObservedMessageSeries, compare_message_series,
    },
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
const TEST_EVALUATION_POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationRequest {
    pub schema_version: u32,
    pub payload: Value,
}

#[async_trait]
pub trait TestScenarioReset: Send + Sync {
    async fn reset(&self) -> Result<(), TestScenarioResetError>;
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TestScenarioResetError {
    #[error("test scenario reset is not configured")]
    Unavailable,
    #[error("test scenario reset failed: {0}")]
    Failed(String),
}

#[derive(Debug, Error)]
pub enum TestRunError {
    #[error(transparent)]
    Repository(#[from] TestRepositoryError),
    #[error(transparent)]
    Reset(#[from] TestScenarioResetError),
    #[error(transparent)]
    Validation(#[from] TestDefinitionValidationError),
    #[error(
        "test definition `{test_id}` references fixture `{actual}`, but Tracer provides `{expected}`"
    )]
    FixtureMismatch {
        test_id: String,
        expected: String,
        actual: String,
    },
    #[error("test setup command {index} was rejected")]
    SetupRejected { index: usize },
    #[error("test command failed: {0}")]
    CommandFailed(String),
    #[error("test correlation closed before evaluation completed")]
    CorrelationClosed,
    #[error(transparent)]
    Submission(#[from] SubmissionError),
    #[error(transparent)]
    Correlation(#[from] CorrelationError),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TestDefinitionValidationError {
    #[error("test definition `{test_id}` has no executable root command")]
    MissingSubject { test_id: String },
    #[error(
        "test definition `{test_id}` declares fixture `{actual}`, but no fixture is configured"
    )]
    FixtureUnavailable { test_id: String, actual: String },
    #[error(
        "test definition `{test_id}` references fixture `{actual}`, but Tracer provides `{expected}`"
    )]
    FixtureMismatch {
        test_id: String,
        expected: String,
        actual: String,
    },
    #[error(
        "test definition `{test_id}` references unknown command `{command}` version {schema_version} for aggregate `{aggregate_type}`"
    )]
    UnknownCommand {
        test_id: String,
        path: String,
        aggregate_type: String,
        command: String,
        schema_version: u32,
    },
    #[error(
        "test definition `{test_id}` has an invalid payload for command `{command}`: {message}"
    )]
    InvalidCommandPayload {
        test_id: String,
        path: String,
        command: String,
        message: String,
    },
    #[error(
        "test definition `{test_id}` has an invalid aggregate ID `{aggregate_id}` for command `{command}`: {message}"
    )]
    InvalidAggregateId {
        test_id: String,
        path: String,
        command: String,
        aggregate_id: String,
        message: String,
    },
    #[error(
        "test definition `{test_id}` payload for command `{command}` is {actual} bytes and exceeds the configured {maximum}-byte limit"
    )]
    CommandPayloadTooLarge {
        test_id: String,
        path: String,
        command: String,
        actual: usize,
        maximum: usize,
    },
}

pub trait TracePayloadPolicy: Send + Sync {
    fn domain_event(&self, event: PredictedDomainEvent) -> PredictedDomainEvent;

    fn rejection(&self, rejection: Value) -> Value;

    fn failure_message(&self, message: String) -> String;

    fn observed_event_payload(&self, _payload: Value) -> Option<Value> {
        None
    }

    fn observed_rejection(
        &self,
        _rejection: MessagingCommandRejection,
    ) -> Result<MessagingCommandRejection, String> {
        let code = ApplicationErrorCode::new("REDACTED").map_err(|error| error.to_string())?;
        MessagingCommandRejection::new(
            CommandRejectionClassification::Internal,
            code,
            "observed rejection redacted",
            None,
        )
        .map_err(|error| error.to_string())
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

    fn observed_rejection(
        &self,
        rejection: MessagingCommandRejection,
    ) -> Result<MessagingCommandRejection, String> {
        Ok(rejection)
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
    test_fixture: Option<String>,
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
            test_fixture: None,
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
    pub fn with_test_fixture(
        mut self,
        name: impl Into<String>,
        reset: Arc<dyn TestScenarioReset>,
    ) -> Self {
        self.test_fixture = Some(name.into());
        self.test_scenario_reset = Some(reset);
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

    pub fn register_json<A, C>(&mut self) -> Result<&mut Self, RuntimeRegistrationError>
    where
        A: Aggregate + CommandHandler<C> + 'static,
        C: CommandDefinition<A> + domain::JsonCommandPayload,
        A::State: Send,
        A::Event: Event + Send,
        <A as CommandHandler<C>>::Rejection: domain::JsonErrorPayload,
    {
        self.bindings.register_json::<A, C>()?;
        Ok(self)
    }

    pub fn register_input_options<A, C, Provider>(
        &mut self,
        provider: Provider,
    ) -> Result<&mut Self, RuntimeRegistrationError>
    where
        A: Aggregate + CommandHandler<C> + 'static,
        C: CommandDefinition<A>,
        A::State: Send,
        A::Event: Event + Send,
        Provider: CommandInputOptions<A, C> + 'static,
    {
        self.bindings
            .register_input_options::<A, C, Provider>(provider)?;
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
        if let Some(repository) = self.test_repository.as_ref() {
            validate_test_repository(
                repository.as_ref(),
                self.test_fixture.as_deref(),
                &self.bindings.simulators,
                test_transport_payload_limit(self.test_transport.as_deref()),
            )?;
        }
        let test_enabled = self.test_event_store.is_some() && self.test_transport.is_some();
        let catalog = build_catalog(
            &self.bindings.registry,
            self.domain_model.as_ref(),
            test_enabled,
            self.dispatch_transport.is_some(),
            self.test_scenario_reset.is_some(),
            self.test_fixture.as_deref(),
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
                test_fixture: self.test_fixture,
                test_repository: self.test_repository,
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
    fixture: Option<&str>,
    simulators: &HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
    maximum_payload_len: usize,
) -> Result<(), RuntimeRegistrationError> {
    for summary in repository.list().items {
        let revision = repository.get(&summary.id).map_err(|error| {
            RuntimeRegistrationError::InvalidTestDefinition {
                id: summary.id.clone(),
                message: error.to_string(),
            }
        })?;
        let definition = &revision.definition;
        validate_test_definition_against_runtime(
            definition,
            fixture,
            simulators,
            maximum_payload_len,
        )
        .map_err(|error| match error {
            TestDefinitionValidationError::FixtureUnavailable { .. } => {
                RuntimeRegistrationError::TestRepositoryWithoutFixture
            }
            error => RuntimeRegistrationError::InvalidTestDefinition {
                id: definition.id().to_owned(),
                message: error.to_string(),
            },
        })?;
    }
    Ok(())
}

fn validate_test_definition_against_runtime(
    definition: &TestDefinition,
    fixture: Option<&str>,
    simulators: &HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
    maximum_payload_len: usize,
) -> Result<(), TestDefinitionValidationError> {
    let fixture = fixture.filter(|fixture| !fixture.trim().is_empty());
    if let Some(setup) = definition.setup() {
        match fixture {
            Some(configured) if configured != setup.fixture => {
                return Err(TestDefinitionValidationError::FixtureMismatch {
                    test_id: definition.id().to_owned(),
                    expected: configured.to_owned(),
                    actual: setup.fixture.clone(),
                });
            }
            None => {
                return Err(TestDefinitionValidationError::FixtureUnavailable {
                    test_id: definition.id().to_owned(),
                    actual: setup.fixture.clone(),
                });
            }
            Some(_) => {}
        }
        for (index, command) in setup.commands.iter().enumerate() {
            validate_test_command(
                definition.id(),
                command,
                &format!("/setup/commands/{index}"),
                simulators,
                maximum_payload_len,
            )?;
        }
    }
    let subject =
        definition
            .subject()
            .ok_or_else(|| TestDefinitionValidationError::MissingSubject {
                test_id: definition.id().to_owned(),
            })?;
    let root_index = definition
        .expected_graph()
        .and_then(|graph| {
            graph
                .nodes()
                .iter()
                .position(|node| node.parent_key().is_none())
        })
        .unwrap_or(0);
    validate_test_command(
        definition.id(),
        &subject.to_test_command(),
        &format!("/expected/graphs/0/nodes/{root_index}"),
        simulators,
        maximum_payload_len,
    )
}

fn validate_test_command(
    test_id: &str,
    command: &TestCommand,
    path: &str,
    simulators: &HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
    maximum_payload_len: usize,
) -> Result<(), TestDefinitionValidationError> {
    AggregateId::new(&command.aggregate.id).map_err(|error| {
        TestDefinitionValidationError::InvalidAggregateId {
            test_id: test_id.to_owned(),
            path: format!("{path}/aggregate/id"),
            command: command.name.clone(),
            aggregate_id: command.aggregate.id.clone(),
            message: error.to_string(),
        }
    })?;
    let key = CommandKey::new(
        &command.aggregate.aggregate_type,
        &command.name,
        command.schema_version,
    );
    let simulator =
        simulators
            .get(&key)
            .ok_or_else(|| TestDefinitionValidationError::UnknownCommand {
                test_id: test_id.to_owned(),
                path: format!("{path}/name"),
                aggregate_type: command.aggregate.aggregate_type.clone(),
                command: command.name.clone(),
                schema_version: command.schema_version,
            })?;
    simulator
        .validate_payload(&command.payload)
        .map_err(
            |message| TestDefinitionValidationError::InvalidCommandPayload {
                test_id: test_id.to_owned(),
                path: format!("{path}/payload"),
                command: command.name.clone(),
                message,
            },
        )?;
    let actual = canonical_json_payload(&command.payload).len();
    if actual > maximum_payload_len {
        return Err(TestDefinitionValidationError::CommandPayloadTooLarge {
            test_id: test_id.to_owned(),
            path: format!("{path}/payload"),
            command: command.name.clone(),
            actual,
            maximum: maximum_payload_len,
        });
    }
    Ok(())
}

fn test_transport_payload_limit(transport: Option<&dyn CommandTransport>) -> usize {
    transport.map_or(MAX_COMMAND_PAYLOAD_LEN, |transport| {
        transport.maximum_payload_len().min(MAX_COMMAND_PAYLOAD_LEN)
    })
}

struct TracerInner {
    history: Arc<dyn EventHistory>,
    test_backing_configured: bool,
    test_transport: Option<Arc<dyn CommandTransport>>,
    dispatch_transport: Option<Arc<dyn CommandTransport>>,
    test_scenario_reset: Option<Arc<dyn TestScenarioReset>>,
    test_fixture: Option<String>,
    test_repository: Option<Arc<dyn TestRepository>>,
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
    correlations: Arc<CorrelationHub>,
    operation_id: String,
    correlation_id: String,
    command: String,
    schema_version: u32,
    aggregate: TestAggregate,
    payload: Value,
    publication: Mutex<Option<CommandPublication>>,
    observation_error: Mutex<Option<CorrelationError>>,
}

impl OperationTransportObserver {
    #[allow(clippy::too_many_arguments)]
    fn new(
        record: Arc<OperationRecord>,
        correlations: Arc<CorrelationHub>,
        operation_id: String,
        correlation_id: String,
        command: String,
        schema_version: u32,
        aggregate: TestAggregate,
        payload: Value,
    ) -> Self {
        Self {
            record,
            correlations,
            operation_id,
            correlation_id,
            command,
            schema_version,
            aggregate,
            payload,
            publication: Mutex::new(None),
            observation_error: Mutex::new(None),
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

    async fn observation_error(&self) -> Option<CorrelationError> {
        self.observation_error.lock().await.clone()
    }
}

#[async_trait]
impl CommandTransportObserver for OperationTransportObserver {
    async fn command_published(&self, publication: CommandPublication) {
        {
            let mut observed = self.publication.lock().await;
            if observed.is_some() {
                return;
            }
            *observed = Some(publication.clone());
        }
        self.record
            .command_published(
                publication.command_message_id().to_owned(),
                publication.duplicate(),
            )
            .await;
        if let Err(error) = self
            .correlations
            .observe_command(
                &self.correlation_id,
                self.operation_id.clone(),
                publication.command_message_id().to_owned(),
                None,
                publication.duplicate(),
                self.command.clone(),
                self.schema_version,
                self.aggregate.clone(),
                Some(self.payload.clone()),
            )
            .await
        {
            *self.observation_error.lock().await = Some(error);
        }
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

struct TestDefinitionEvaluation {
    operation: OperationSnapshot,
    observed: ObservedMessageSeries,
    comparison: MessageSeriesComparison,
}

struct OperationSubmission {
    snapshot: OperationSnapshot,
    correlation_lease: Option<CorrelationSubscription>,
}

impl Tracer {
    pub fn catalog(&self) -> &TracerCatalog {
        &self.inner.catalog
    }

    pub fn test_definitions(&self) -> Result<TestDefinitionCollection, TestRepositoryError> {
        self.inner
            .test_repository
            .as_ref()
            .map(|repository| repository.list())
            .ok_or(TestRepositoryError::Unavailable)
    }

    pub fn test_definition(
        &self,
        test_id: &str,
    ) -> Result<TestDefinitionRevision, TestRepositoryError> {
        self.inner
            .test_repository
            .as_ref()
            .ok_or(TestRepositoryError::Unavailable)?
            .get(test_id)
    }

    pub fn validate_test_definition(
        &self,
        definition: &TestDefinition,
    ) -> Result<(), TestDefinitionValidationError> {
        validate_test_definition_against_runtime(
            definition,
            self.inner.test_fixture.as_deref(),
            &self.inner.simulators,
            test_transport_payload_limit(self.inner.test_transport.as_deref()),
        )
    }

    pub async fn run_inline_test(
        &self,
        definition: TestDefinition,
    ) -> Result<TestReport, TestRunError> {
        self.run_test_definition(definition, None).await
    }

    pub async fn run_test(&self, test_id: &str) -> Result<TestReport, TestRunError> {
        let revision = self.test_definition(test_id)?;
        self.run_test_definition(revision.definition, Some(revision.revision))
            .await
    }

    #[allow(clippy::too_many_lines)]
    async fn run_test_definition(
        &self,
        definition: TestDefinition,
        revision: Option<String>,
    ) -> Result<TestReport, TestRunError> {
        self.validate_test_definition(&definition)?;
        let graph = definition.expected_graph().cloned().ok_or_else(|| {
            TestRunError::CommandFailed("test definition has no executable graph".to_owned())
        })?;
        let subject = definition
            .subject()
            .map(crate::ExpectedCommandFields::to_test_command)
            .ok_or_else(|| {
                TestRunError::CommandFailed("test definition has no executable subject".to_owned())
            })?;
        let expected = definition.expected().clone();
        let within = graph.effective_within(&expected).as_duration();
        let settle_for = graph.effective_settle_for(&expected).as_duration();
        let setup_commands = definition
            .setup()
            .map(|setup| setup.commands.clone())
            .unwrap_or_default();
        let test_id = definition.id().to_owned();

        let sequence = self.inner.test_run_sequence.fetch_add(1, Ordering::Relaxed);
        let run_id = format!("test-run-{sequence}");
        let _test_run = self.inner.test_run_gate.lock().await;
        if !self.inner.test_backing_configured || self.inner.test_transport.is_none() {
            return Err(SubmissionError::ModeUnavailable("test").into());
        }
        if self.inner.test_scenario_reset.is_none() {
            return Err(TestScenarioResetError::Unavailable.into());
        }
        let deadline = run_deadline(within)?;
        tokio::time::timeout_at(deadline, self.reset_test_scenario_unlocked())
            .await
            .map_err(|_| {
                TestScenarioResetError::Failed(
                    "the behavioral test deadline elapsed during reset".to_owned(),
                )
            })??;

        for (index, command) in setup_commands.iter().enumerate() {
            self.execute_setup_command(
                command,
                deadline,
                &format!("{run_id}-setup-{index}"),
                index,
            )
            .await?;
        }

        let evaluation = self
            .evaluate_test_subject(
                &subject,
                &graph,
                deadline,
                settle_for,
                &format!("{run_id}-subject"),
            )
            .await?;
        self.build_test_report(run_id, test_id, revision, expected, evaluation)
    }

    async fn execute_setup_command(
        &self,
        command: &TestCommand,
        deadline: tokio::time::Instant,
        idempotency_key: &str,
        index: usize,
    ) -> Result<(), TestRunError> {
        let queued = tokio::time::timeout_at(
            deadline,
            self.submit_test_unlocked(
                &command.aggregate.aggregate_type,
                &command.aggregate.id,
                &command.name,
                SimulationRequest {
                    schema_version: command.schema_version,
                    payload: command.payload.clone(),
                },
                Some(idempotency_key),
            ),
        )
        .await
        .map_err(|_| {
            TestRunError::CommandFailed(format!(
                "test setup command {index} could not be submitted before the deadline"
            ))
        })??;
        let record = self.record(&queued.operation_id).await?;

        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                if !record.is_terminal() {
                    self.abort_operation_for_deadline(&queued.operation_id)
                        .await;
                }
                return Err(TestRunError::CommandFailed(format!(
                    "test setup command {index} did not complete before the deadline"
                )));
            }
            let snapshot = record.snapshot().await;
            match snapshot.status {
                crate::OperationStatus::Completed => match snapshot.result {
                    Some(OperationResult::Accepted { .. }) => return Ok(()),
                    Some(OperationResult::Rejected { .. }) => {
                        return Err(TestRunError::SetupRejected { index });
                    }
                    None => {
                        return Err(TestRunError::CommandFailed(
                            "setup operation completed without a result".to_owned(),
                        ));
                    }
                },
                crate::OperationStatus::Failed | crate::OperationStatus::Indeterminate => {
                    return Err(operation_run_error(&snapshot));
                }
                crate::OperationStatus::Queued | crate::OperationStatus::Running => {}
            }
            tokio::time::sleep_until(next_poll_at(now, deadline)).await;
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn evaluate_test_subject(
        &self,
        command: &TestCommand,
        graph: &MessageGraphDefinition,
        deadline: tokio::time::Instant,
        settle_for: Duration,
        idempotency_key: &str,
    ) -> Result<TestDefinitionEvaluation, TestRunError> {
        let (queued, _correlation_lease) = tokio::time::timeout_at(
            deadline,
            self.submit_test_unlocked_pinned(
                &command.aggregate.aggregate_type,
                &command.aggregate.id,
                &command.name,
                SimulationRequest {
                    schema_version: command.schema_version,
                    payload: command.payload.clone(),
                },
                Some(idempotency_key),
            ),
        )
        .await
        .map_err(|_| {
            TestRunError::CommandFailed(
                "test subject could not be submitted before the deadline".to_owned(),
            )
        })??;
        let record = self.record(&queued.operation_id).await?;
        let mut context = MessageSeriesComparisonContext::default();
        let mut settle_deadline = None;
        let mut last_comparison = None;

        loop {
            let operation = record.snapshot().await;
            let evidence = self
                .observed_evidence_snapshot(&queued.correlation_id)
                .await?;
            let now = tokio::time::Instant::now();

            match operation.status {
                crate::OperationStatus::Failed | crate::OperationStatus::Indeterminate => {
                    return Err(operation_run_error(&operation));
                }
                crate::OperationStatus::Queued
                | crate::OperationStatus::Running
                | crate::OperationStatus::Completed => {}
            }

            if evidence.failure.is_some() {
                return Ok(TestDefinitionEvaluation {
                    operation,
                    observed: evidence.observed.clone(),
                    comparison: evidence_failure_comparison(&evidence),
                });
            }

            if now >= deadline {
                return self
                    .timed_out_test_evaluation(
                        &record,
                        &queued.operation_id,
                        &queued.correlation_id,
                        settle_deadline.is_some(),
                        false,
                        last_comparison,
                    )
                    .await;
            }

            if let Some(settle_deadline) = settle_deadline
                && settle_deadline <= deadline
                && now >= settle_deadline
            {
                context.settle_completed_at_order =
                    Some(next_observation_order(&evidence.observed));
                let Some(comparison) =
                    compare_evidence_snapshot_until(graph, &evidence, context, deadline).await?
                else {
                    return self
                        .timed_out_test_evaluation(
                            &record,
                            &queued.operation_id,
                            &queued.correlation_id,
                            true,
                            true,
                            last_comparison,
                        )
                        .await;
                };
                if self
                    .inner
                    .correlations
                    .evidence_revision(&queued.correlation_id)?
                    != evidence.revision
                {
                    continue;
                }
                return Ok(TestDefinitionEvaluation {
                    operation,
                    observed: evidence.observed,
                    comparison,
                });
            }

            let Some(comparison) =
                compare_evidence_snapshot_until(graph, &evidence, context, deadline).await?
            else {
                return self
                    .timed_out_test_evaluation(
                        &record,
                        &queued.operation_id,
                        &queued.correlation_id,
                        settle_deadline.is_some(),
                        true,
                        last_comparison,
                    )
                    .await;
            };
            last_comparison = Some((evidence.revision, comparison.clone()));

            if settle_deadline.is_some()
                && comparison.status == MessageSeriesComparisonStatus::Failed
            {
                return Ok(TestDefinitionEvaluation {
                    operation,
                    observed: evidence.observed,
                    comparison,
                });
            }

            if comparison
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "comparison-work-limit-exceeded")
            {
                return Ok(TestDefinitionEvaluation {
                    operation,
                    observed: evidence.observed,
                    comparison,
                });
            }

            if operation.status == crate::OperationStatus::Completed
                && comparison
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "command-outcome-mismatch")
            {
                return Ok(TestDefinitionEvaluation {
                    operation,
                    observed: evidence.observed,
                    comparison,
                });
            }

            if settle_deadline.is_none()
                && operation.status == crate::OperationStatus::Completed
                && comparison.status == MessageSeriesComparisonStatus::Passed
            {
                context.settle_started_at_order = Some(next_observation_order(&evidence.observed));
                settle_deadline = Some(settle_deadline_at(now, settle_for)?);
                continue;
            }

            let mut wake = next_poll_at(now, deadline);
            if let Some(settle_deadline) = settle_deadline {
                wake = wake.min(settle_deadline);
            }
            tokio::time::sleep_until(wake).await;
        }
    }

    async fn timed_out_test_evaluation(
        &self,
        record: &Arc<OperationRecord>,
        operation_id: &str,
        correlation_id: &str,
        settling: bool,
        comparison_timed_out: bool,
        last_comparison: Option<(u64, MessageSeriesComparison)>,
    ) -> Result<TestDefinitionEvaluation, TestRunError> {
        if !record.is_terminal() {
            self.abort_operation_for_deadline(operation_id).await;
        }
        let operation = record.snapshot().await;
        let evidence = self.observed_evidence_snapshot(correlation_id).await?;
        // Diagnostics must describe the same evidence serialized into the report.
        let mut comparison = last_comparison
            .filter(|(revision, _)| *revision == evidence.revision)
            .map_or_else(
                || evidence_failure_comparison(&evidence),
                |(_, comparison)| comparison,
            );
        add_deadline_diagnostic(&mut comparison, settling);
        if comparison_timed_out {
            comparison
                .diagnostics
                .push(MessageSeriesComparisonDiagnostic {
                    code: "comparison-deadline-exceeded",
                    path: "/expected".to_owned(),
                    message:
                        "message-series comparison did not complete before the overall deadline"
                            .to_owned(),
                    expected: None,
                    observed: None,
                });
        }
        Ok(TestDefinitionEvaluation {
            operation,
            observed: evidence.observed,
            comparison,
        })
    }

    async fn observed_evidence_snapshot(
        &self,
        correlation_id: &str,
    ) -> Result<CorrelationEvidenceSnapshot, CorrelationError> {
        self.inner
            .correlations
            .evidence_snapshot(correlation_id)
            .await
    }

    fn build_test_report(
        &self,
        run_id: String,
        test_id: String,
        revision: Option<String>,
        expected: MessageSeriesDefinition,
        evaluation: TestDefinitionEvaluation,
    ) -> Result<TestReport, TestRunError> {
        let TestDefinitionEvaluation {
            operation,
            observed,
            mut comparison,
        } = evaluation;
        let observed =
            redact_observed_message_series(&observed, self.inner.trace_payload_policy.as_ref())
                .map_err(TestRunError::CommandFailed)?;
        redact_comparison(&mut comparison, self.inner.trace_payload_policy.as_ref())
            .map_err(TestRunError::CommandFailed)?;
        let command_message_id = operation_command_message_id(&operation)
            .map(str::to_owned)
            .or_else(|| {
                let root_key = expected.graphs().first()?.root_command()?.key;
                comparison
                    .matches
                    .iter()
                    .find(|matched| matched.expected_key == root_key)
                    .map(|matched| matched.observed_message_id.clone())
            });
        let command_outcome = command_message_id
            .as_deref()
            .and_then(|message_id| observed.command_outcome(message_id))
            .cloned();
        let status = match comparison.status {
            MessageSeriesComparisonStatus::Passed => TestReportStatus::Passed,
            MessageSeriesComparisonStatus::Failed => TestReportStatus::Failed,
        };
        let operation_id = operation.operation_id.clone();
        let correlation_id = operation.correlation_id.clone();
        Ok(TestReport {
            run_id,
            test_id,
            revision,
            status,
            expected,
            observed,
            comparison,
            command_outcome,
            operation_id: operation_id.clone(),
            correlation_id: correlation_id.clone(),
            operation_href: format!("/operations/{operation_id}"),
            operation_events_href: format!("/operations/{operation_id}/events"),
            correlation_events_href: format!("/correlations/{correlation_id}/events"),
            operation,
        })
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

    pub async fn reset_test_scenario(&self) -> Result<(), TestScenarioResetError> {
        let _test_run = self.inner.test_run_gate.lock().await;
        self.reset_test_scenario_unlocked().await
    }

    async fn reset_test_scenario_unlocked(&self) -> Result<(), TestScenarioResetError> {
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
        let result = reset.reset().await;
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

    async fn submit_test_unlocked_pinned(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<(OperationSnapshot, CorrelationSubscription), SubmissionError> {
        let submission = self
            .submit_operation_internal(
                OperationMode::Test,
                aggregate_type,
                aggregate_id,
                command,
                request,
                idempotency_key,
                true,
            )
            .await?;
        let lease = submission.correlation_lease.ok_or_else(|| {
            SubmissionError::InvalidOperationId(
                "subject correlation lease was not acquired".to_owned(),
            )
        })?;
        Ok((submission.snapshot, lease))
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

    async fn submit_operation(
        &self,
        mode: OperationMode,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OperationSnapshot, SubmissionError> {
        self.submit_operation_internal(
            mode,
            aggregate_type,
            aggregate_id,
            command,
            request,
            idempotency_key,
            false,
        )
        .await
        .map(|submission| submission.snapshot)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the reset guard must remain held until the spawned operation completes"
    )]
    async fn submit_operation_internal(
        &self,
        mode: OperationMode,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
        pin_correlation: bool,
    ) -> Result<OperationSubmission, SubmissionError> {
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
                let correlation_lease = if pin_correlation {
                    Some(
                        self.inner
                            .correlations
                            .subscribe(&correlation_id, 0)
                            .await
                            .map_err(|error| {
                                SubmissionError::InvalidOperationId(error.to_string())
                            })?,
                    )
                } else {
                    None
                };
                drop(operations);
                return Ok(OperationSubmission {
                    snapshot,
                    correlation_lease,
                });
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
            let correlation_lease = if pin_correlation {
                match self.inner.correlations.subscribe(&correlation_id, 0).await {
                    Ok(lease) => Some(lease),
                    Err(error) => {
                        operations.records.remove(&operation_key);
                        operations
                            .insertion_order
                            .retain(|retained| retained != &operation_key);
                        self.inner.correlations.remove_if_inactive(&correlation_id);
                        drop(operations);
                        return Err(SubmissionError::InvalidOperationId(error.to_string()));
                    }
                }
            } else {
                None
            };
            drop(operations);
            (permit, correlation_lease)
        };

        let (permit, correlation_lease) = permit;

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
            if let Err(error) = execution.await {
                let (code, message) = operation_task_failure(&error);
                panic_record
                    .fail_after_possible_publication(code, message.to_owned())
                    .await;
                panic_tracer
                    .record_correlation_result(&panic_correlation_id, &panic_record)
                    .await;
            }
        });
        Ok(OperationSubmission {
            snapshot: queued,
            correlation_lease,
        })
    }

    pub async fn operation(
        &self,
        operation_id: &str,
    ) -> Result<OperationSnapshot, SubmissionError> {
        let record = self.record(operation_id).await?;
        Ok(record.snapshot().await)
    }

    async fn abort_operation_for_deadline(&self, operation_id: &str) {
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
                correlation_id.clone(),
                execution_fingerprint,
                aggregate_type.clone(),
                aggregate_id.clone(),
                command.clone(),
                schema_version,
                payload.clone(),
            );
            let observer = Arc::new(OperationTransportObserver::new(
                Arc::clone(&record),
                Arc::clone(&self.inner.correlations),
                invocation.operation_id().as_str().to_owned(),
                correlation_id.clone(),
                command,
                schema_version,
                TestAggregate {
                    aggregate_type,
                    id: aggregate_id.as_str().to_owned(),
                },
                payload,
            ));
            match transport.invoke(invocation, observer.clone()).await {
                Ok(receipt) if observer.matches(&receipt).await => {
                    if let Some(error) = observer.observation_error().await {
                        record
                            .fail_after_possible_publication(
                                "invalid-command-observation",
                                bounded_failure_message(
                                    self.inner.trace_payload_policy.as_ref(),
                                    error.to_string(),
                                    self.inner.maximum_operation_payload_bytes,
                                ),
                            )
                            .await;
                        return;
                    }
                    let outcome = match command_response_outcome(&receipt) {
                        Ok(outcome) => outcome,
                        Err(message) => {
                            record
                                .fail_after_possible_publication(
                                    "invalid-command-transport-response",
                                    bounded_failure_message(
                                        self.inner.trace_payload_policy.as_ref(),
                                        message,
                                        self.inner.maximum_operation_payload_bytes,
                                    ),
                                )
                                .await;
                            return;
                        }
                    };
                    if let Err(error) = self
                        .inner
                        .correlations
                        .observe_command_outcome(
                            &correlation_id,
                            receipt.response_message_id().to_owned(),
                            receipt.command_message_id().to_owned(),
                            outcome,
                        )
                        .await
                    {
                        record
                            .fail_after_possible_publication(
                                "invalid-command-outcome-observation",
                                bounded_failure_message(
                                    self.inner.trace_payload_policy.as_ref(),
                                    error.to_string(),
                                    self.inner.maximum_operation_payload_bytes,
                                ),
                            )
                            .await;
                        return;
                    }
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
                    let message_id = ContentFingerprint::digest(format!(
                        "{correlation_id}:predicted-domain-event:{}",
                        event.ordinal
                    ))
                    .to_hex();
                    let mut observation = DomainEventObservation::new(
                        message_id,
                        event.event_type.clone(),
                        event.schema_version,
                    )
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

fn operation_run_error(operation: &OperationSnapshot) -> TestRunError {
    let message = operation.failure.as_ref().map_or_else(
        || format!("operation ended with status `{:?}`", operation.status),
        |failure| format!("{}: {}", failure.code, failure.message),
    );
    TestRunError::CommandFailed(message)
}

fn operation_task_failure(error: &tokio::task::JoinError) -> (&'static str, &'static str) {
    if error.is_cancelled() {
        (
            "operation-cancelled",
            "the command operation task was cancelled",
        )
    } else {
        ("operation-panicked", "the command operation task panicked")
    }
}

fn run_deadline(within: Duration) -> Result<tokio::time::Instant, TestRunError> {
    tokio::time::Instant::now()
        .checked_add(within)
        .ok_or_else(|| {
            TestRunError::CommandFailed("within exceeds the supported timer range".to_owned())
        })
}

fn settle_deadline_at(
    now: tokio::time::Instant,
    settle_for: Duration,
) -> Result<tokio::time::Instant, TestRunError> {
    now.checked_add(settle_for).ok_or_else(|| {
        TestRunError::CommandFailed("settleFor exceeds the supported timer range".to_owned())
    })
}

fn next_poll_at(now: tokio::time::Instant, deadline: tokio::time::Instant) -> tokio::time::Instant {
    now.checked_add(TEST_EVALUATION_POLL_INTERVAL)
        .map_or(deadline, |poll| poll.min(deadline))
}

fn compare_evidence_snapshot(
    graph: &MessageGraphDefinition,
    evidence: &CorrelationEvidenceSnapshot,
    context: MessageSeriesComparisonContext,
) -> MessageSeriesComparison {
    let mut comparison = compare_message_series(graph, &evidence.observed, context);
    append_evidence_diagnostics(&mut comparison, evidence);
    comparison
}

async fn compare_evidence_snapshot_until(
    graph: &MessageGraphDefinition,
    evidence: &CorrelationEvidenceSnapshot,
    context: MessageSeriesComparisonContext,
    deadline: tokio::time::Instant,
) -> Result<Option<MessageSeriesComparison>, TestRunError> {
    let graph = graph.clone();
    let evidence = evidence.clone();
    let comparison =
        tokio::task::spawn_blocking(move || compare_evidence_snapshot(&graph, &evidence, context));
    match tokio::time::timeout_at(deadline, comparison).await {
        Ok(Ok(comparison)) => Ok(Some(comparison)),
        Ok(Err(error)) => Err(TestRunError::CommandFailed(format!(
            "message-series comparison task failed: {error}"
        ))),
        Err(_) => Ok(None),
    }
}

fn evidence_failure_comparison(evidence: &CorrelationEvidenceSnapshot) -> MessageSeriesComparison {
    let mut comparison = MessageSeriesComparison {
        status: MessageSeriesComparisonStatus::Failed,
        matches: Vec::new(),
        diagnostics: Vec::new(),
    };
    append_evidence_diagnostics(&mut comparison, evidence);
    comparison
}

fn append_evidence_diagnostics(
    comparison: &mut MessageSeriesComparison,
    evidence: &CorrelationEvidenceSnapshot,
) {
    for conflict in &evidence.conflicts {
        comparison
            .diagnostics
            .push(MessageSeriesComparisonDiagnostic {
                code: "observation-conflict",
                path: "/observed".to_owned(),
                message: format!(
                    "conflicting observations were recorded for `{}`: {}",
                    conflict.identity, conflict.message
                ),
                expected: conflict.existing.clone(),
                observed: conflict.observed.clone(),
            });
    }
    if !evidence.conflicts.is_empty() {
        comparison.status = MessageSeriesComparisonStatus::Failed;
    }
    if let Some(failure) = &evidence.failure {
        comparison
            .diagnostics
            .push(MessageSeriesComparisonDiagnostic {
                code: "observation-failure",
                path: "/observed".to_owned(),
                message: format!(
                    "failed to retain correlated observation `{}` ({} occurrence(s)): {}",
                    failure.identity, failure.count, failure.message
                ),
                expected: None,
                observed: None,
            });
        comparison.status = MessageSeriesComparisonStatus::Failed;
    }
}

fn next_observation_order(observed: &ObservedMessageSeries) -> u64 {
    observed
        .messages()
        .iter()
        .map(crate::ObservedMessageNode::observation_order)
        .chain(
            observed
                .command_outcomes()
                .iter()
                .map(ObservedCommandOutcome::observation_order),
        )
        .max()
        .map_or(0, |order| order.saturating_add(1))
}

fn add_deadline_diagnostic(comparison: &mut MessageSeriesComparison, settling: bool) {
    if settling {
        comparison
            .diagnostics
            .push(MessageSeriesComparisonDiagnostic {
                code: "timeout-during-settle",
                path: "/expected/settleFor".to_owned(),
                message: "the overall deadline elapsed before settling completed".to_owned(),
                expected: None,
                observed: None,
            });
    } else if !comparison
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code.starts_with("timeout"))
    {
        comparison
            .diagnostics
            .push(MessageSeriesComparisonDiagnostic {
                code: "timeout-before-expectations",
                path: "/expected/within".to_owned(),
                message: "the expected behavior did not complete before the deadline".to_owned(),
                expected: None,
                observed: None,
            });
    }
    comparison.status = MessageSeriesComparisonStatus::Failed;
}

fn operation_command_message_id(operation: &OperationSnapshot) -> Option<&str> {
    match operation.result.as_ref()? {
        OperationResult::Accepted {
            command_message_id, ..
        }
        | OperationResult::Rejected {
            command_message_id, ..
        } => command_message_id.as_deref(),
    }
}

fn redact_observed_message_series(
    observed: &ObservedMessageSeries,
    policy: &dyn TracePayloadPolicy,
) -> Result<ObservedMessageSeries, String> {
    let mut wire = serde_json::to_value(observed).map_err(|error| error.to_string())?;
    let object = wire
        .as_object_mut()
        .ok_or_else(|| "observed message series did not serialize as an object".to_owned())?;
    let messages = object
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "observed messages did not serialize as an array".to_owned())?;
    for message in messages {
        redact_wire_payload(message, policy);
    }
    let outcomes = object
        .get_mut("commandOutcomes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "observed command outcomes did not serialize as an array".to_owned())?;
    for outcome in outcomes {
        if let Some(outcome) = outcome.get_mut("outcome") {
            redact_wire_command_outcome(outcome, policy)?;
        }
    }
    serde_json::from_value(wire).map_err(|error| error.to_string())
}

fn redact_comparison(
    comparison: &mut MessageSeriesComparison,
    policy: &dyn TracePayloadPolicy,
) -> Result<(), String> {
    for diagnostic in &mut comparison.diagnostics {
        match diagnostic.code {
            "payload-mismatch" => {
                diagnostic.observed = diagnostic
                    .observed
                    .take()
                    .and_then(|payload| policy.observed_event_payload(payload));
            }
            "unexpected-observed" => {
                if let Some(observed) = diagnostic.observed.as_mut() {
                    redact_wire_payload(observed, policy);
                }
            }
            "command-outcome-mismatch" => {
                if let Some(observed) = diagnostic.observed.as_mut() {
                    redact_wire_command_outcome(observed, policy)?;
                }
            }
            "observation-conflict" => {
                if let Some(existing) = diagnostic.expected.as_mut() {
                    redact_wire_observation(existing, policy)?;
                }
                if let Some(observed) = diagnostic.observed.as_mut() {
                    redact_wire_observation(observed, policy)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn redact_wire_payload(value: &mut Value, policy: &dyn TracePayloadPolicy) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(payload) = object.remove("payload") else {
        return;
    };
    if let Some(payload) = policy.observed_event_payload(payload) {
        object.insert("payload".to_owned(), payload);
    }
}

fn redact_wire_observation(
    value: &mut Value,
    policy: &dyn TracePayloadPolicy,
) -> Result<(), String> {
    redact_wire_payload(value, policy);
    if let Some(outcome) = value.get_mut("outcome") {
        redact_wire_command_outcome(outcome, policy)?;
    }
    Ok(())
}

fn redact_wire_command_outcome(
    value: &mut Value,
    policy: &dyn TracePayloadPolicy,
) -> Result<(), String> {
    if value.get("status").and_then(Value::as_str) != Some("rejected") {
        return Ok(());
    }
    let Some(rejection) = value.get_mut("value").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let typed =
        serde_json::from_value::<MessagingCommandRejection>(Value::Object(rejection.clone()))
            .map_err(|error| error.to_string())?;
    let filtered = policy.observed_rejection(typed)?;
    *rejection = serde_json::to_value(filtered)
        .map_err(|error| error.to_string())?
        .as_object()
        .cloned()
        .ok_or_else(|| "observed rejection did not serialize as an object".to_owned())?;
    Ok(())
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

fn command_response_outcome(receipt: &CommandReceipt) -> Result<CommandResponseOutcome, String> {
    let CommandOutcome::Rejected(rejection) = receipt.outcome() else {
        return Ok(CommandResponseOutcome::Accepted);
    };
    let classification = match rejection.classification.as_str() {
        "invalid_request" | "invalid-request" => CommandRejectionClassification::InvalidRequest,
        "unauthorized" => CommandRejectionClassification::Unauthorized,
        "forbidden" => CommandRejectionClassification::Forbidden,
        "not_found" | "not-found" => CommandRejectionClassification::NotFound,
        "conflict" => CommandRejectionClassification::Conflict,
        "rate_limited" | "rate-limited" => CommandRejectionClassification::RateLimited,
        "unavailable" => CommandRejectionClassification::Unavailable,
        "internal" => CommandRejectionClassification::Internal,
        classification => {
            return Err(format!(
                "command transport returned unknown rejection classification `{classification}`"
            ));
        }
    };
    let code = ApplicationErrorCode::new(rejection.code.clone()).map_err(|error| {
        format!("command transport returned an invalid rejection code: {error}")
    })?;
    let rejection = MessagingCommandRejection::new(
        classification,
        code,
        rejection.message.clone(),
        rejection.details.clone(),
    )
    .map_err(|error| format!("command transport returned an invalid rejection: {error}"))?;
    Ok(CommandResponseOutcome::Rejected(rejection))
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
    use serde_json::json;

    use super::*;

    fn command_series_definition(payload: &Value, outcome: &Value) -> MessageSeriesDefinition {
        serde_json::from_value(json!({
            "within": "2s",
            "settleFor": "1ms",
            "graphs": [{
                "nodes": [{
                    "kind": "command",
                    "key": "subject",
                    "name": "rent-bicycle",
                    "schemaVersion": 1,
                    "aggregate": {
                        "type": "rental/bicycle",
                        "id": "bike-1"
                    },
                    "payload": payload,
                    "outcome": outcome
                }]
            }]
        }))
        .expect("valid command message series")
    }

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

    #[test]
    fn transport_rejection_converts_to_the_exact_messaging_outcome() {
        let receipt = CommandReceipt::rejected(
            "command-1",
            "response-1",
            false,
            crate::CommandRejection::new(
                "conflict",
                "rental.already_rented",
                "The bicycle is already rented.",
                Some(json!({ "bicycleId": "bike-1" })),
            ),
        );

        let CommandResponseOutcome::Rejected(rejection) =
            command_response_outcome(&receipt).expect("valid transport rejection")
        else {
            panic!("expected a rejected messaging outcome");
        };
        assert_eq!(
            rejection.classification(),
            CommandRejectionClassification::Conflict
        );
        assert_eq!(rejection.code().as_str(), "rental.already_rented");
        assert_eq!(rejection.message(), "The bicycle is already rented.");
        assert_eq!(rejection.details(), Some(&json!({ "bicycleId": "bike-1" })));
    }

    #[test]
    fn report_comparison_uses_raw_rejection_before_typed_redaction() {
        let aggregate = TestAggregate {
            aggregate_type: "rental/bicycle".to_owned(),
            id: "bike-1".to_owned(),
        };
        let payload = json!({
            "metadata": { "payload": { "application": true } },
            "secret": true
        });
        let rejection = MessagingCommandRejection::new(
            CommandRejectionClassification::Conflict,
            ApplicationErrorCode::new("rental.already_rented").expect("valid code"),
            "The bicycle is already rented.",
            Some(json!({ "private": true })),
        )
        .expect("valid rejection");
        let observed = ObservedMessageSeries::try_from_parts(
            [crate::ObservedMessageNode::command(
                "command-1",
                "correlation-1",
                None,
                "rent-bicycle",
                1,
                aggregate,
                Some(payload.clone()),
            )],
            [ObservedCommandOutcome::try_new(
                "response-1",
                "command-1",
                "correlation-1",
                CommandResponseOutcome::Rejected(rejection),
            )
            .expect("valid outcome")],
        )
        .expect("valid observed series");

        let definition = command_series_definition(
            &payload,
            &json!({ "rejected": { "code": "rental.already_rented" } }),
        );
        assert_eq!(
            compare_message_series(
                &definition.graphs()[0],
                &observed,
                MessageSeriesComparisonContext::default(),
            )
            .status,
            MessageSeriesComparisonStatus::Passed
        );

        let redacted = redact_observed_message_series(&observed, &RedactTracePayloads)
            .expect("redacted series");
        assert_eq!(
            redacted.messages().get("command-1").unwrap().payload(),
            None
        );
        let CommandResponseOutcome::Rejected(rejection) = redacted.command_outcomes()[0].outcome()
        else {
            panic!("expected a rejected outcome");
        };
        assert_eq!(
            rejection.classification(),
            CommandRejectionClassification::Internal
        );
        assert_eq!(rejection.code().as_str(), "REDACTED");
        assert_eq!(rejection.message(), "observed rejection redacted");
        assert_eq!(rejection.details(), None);
        assert_eq!(
            compare_message_series(
                &definition.graphs()[0],
                &redacted,
                MessageSeriesComparisonContext::default(),
            )
            .status,
            MessageSeriesComparisonStatus::Failed
        );

        let exposed =
            redact_observed_message_series(&observed, &ExposeTracePayloadsForLocalDevelopment)
                .expect("exposed series");
        assert_eq!(
            exposed.messages().get("command-1").unwrap().payload(),
            Some(&payload)
        );
        let CommandResponseOutcome::Rejected(rejection) = exposed.command_outcomes()[0].outcome()
        else {
            panic!("expected a rejected outcome");
        };
        assert_eq!(
            rejection.classification(),
            CommandRejectionClassification::Conflict
        );
        assert_eq!(rejection.code().as_str(), "rental.already_rented");
        assert_eq!(rejection.message(), "The bicycle is already rented.");
        assert_eq!(rejection.details(), Some(&json!({ "private": true })));
    }

    #[test]
    fn observation_conflicts_force_a_failed_comparison() {
        let aggregate = TestAggregate {
            aggregate_type: "rental/bicycle".to_owned(),
            id: "bike-1".to_owned(),
        };
        let observed = ObservedMessageSeries::try_from_parts(
            [crate::ObservedMessageNode::command(
                "command-1",
                "correlation-1",
                None,
                "rent-bicycle",
                1,
                aggregate,
                Some(json!({ "bicycleId": "bike-1" })),
            )],
            [ObservedCommandOutcome::try_new(
                "response-1",
                "command-1",
                "correlation-1",
                CommandResponseOutcome::Accepted,
            )
            .expect("valid outcome")],
        )
        .expect("valid observed series");
        let definition =
            command_series_definition(&json!({ "bicycleId": "bike-1" }), &json!("accepted"));
        let graph = &definition.graphs()[0];
        assert_eq!(
            compare_message_series(graph, &observed, MessageSeriesComparisonContext::default())
                .status,
            MessageSeriesComparisonStatus::Passed
        );

        let comparison = compare_evidence_snapshot(
            graph,
            &CorrelationEvidenceSnapshot {
                observed: observed.clone(),
                conflicts: vec![crate::correlation::CorrelationObservationConflict {
                    identity: "message:command-1".to_owned(),
                    message: "same message ID carried different evidence".to_owned(),
                    existing: Some(json!({ "payload": { "bicycleId": "bike-1" } })),
                    observed: Some(json!({ "payload": { "bicycleId": "bike-2" } })),
                }],
                failure: None,
                revision: 1,
            },
            MessageSeriesComparisonContext::default(),
        );

        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Failed);
        assert!(
            comparison
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "observation-conflict")
        );

        let comparison = compare_evidence_snapshot(
            graph,
            &CorrelationEvidenceSnapshot {
                observed,
                conflicts: Vec::new(),
                failure: Some(crate::correlation::CorrelationObservationFailure {
                    identity: "event-2".to_owned(),
                    message: "stored event checksum is invalid".to_owned(),
                    count: 1,
                }),
                revision: 1,
            },
            MessageSeriesComparisonContext::default(),
        );
        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Failed);
        assert!(
            comparison
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "observation-failure")
        );
    }

    #[tokio::test]
    async fn cancelled_operation_tasks_have_a_distinct_terminal_code() {
        let task = tokio::spawn(std::future::pending::<()>());
        task.abort();
        let error = task
            .await
            .expect_err("aborted task must return a join error");
        let (code, message) = operation_task_failure(&error);
        let record = OperationRecord::new(NewOperation {
            operation_id: "operation-1".to_owned(),
            correlation_id: "correlation-1".to_owned(),
            fingerprint: "fingerprint".to_owned(),
            mode: OperationMode::Test,
            command: "rent-bicycle",
            schema_version: 1,
            aggregate_type: "rental/bicycle",
            aggregate_id: "bike-1",
        });
        record.start().await;
        record
            .fail_after_possible_publication(code, message.to_owned())
            .await;

        let snapshot = record.snapshot().await;
        assert_eq!(snapshot.status, crate::OperationStatus::Failed);
        assert_eq!(snapshot.failure.expect("terminal failure").code, code);
    }
}
