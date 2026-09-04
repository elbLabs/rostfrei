mod behavioral;
mod catalog;
mod command_bus;
mod correlation;
mod input;
mod message_series;
mod operation;
mod runtime;
mod service;
pub mod transport;

#[cfg(feature = "http")]
pub mod http;

pub use behavioral::{
    FIXTURE_OPERATION_ID_PREFIX, FilesystemTestRepository, MAX_EXPOSED_FIXTURE_PAYLOAD_BYTES,
    MAX_FIXTURE_EVENTS, MAX_FIXTURE_PAYLOAD_BYTES, MAX_FIXTURE_STREAMS, MaterializedFixtureEvent,
    MaterializedFixtureStream, MaterializedTestFixture, ResolvedTestDefinition, TestAggregate,
    TestCommand, TestDefinition, TestDefinitionCollection, TestDefinitionRevision,
    TestDefinitionSummary, TestExpectationResult, TestFixture, TestFixtureError, TestFixtureEvent,
    TestFixtureStream, TestGiven, TestOutcome, TestRejection, TestReport, TestReportFailure,
    TestReportStatus, TestRepository, TestRepositoryError, TestThen, TestTimeout,
    TestTimeoutParseError, TestTrace, TestWhen, TraceExpectation, outcome_matches,
    payload_matches_subset, trace_expectation_matches,
};
pub use catalog::{
    AggregateInstanceCollection, AggregateInstanceSummary, CatalogAggregate, CatalogCommand,
    CatalogCommandVersion, CatalogContext, CatalogTestRepository, CatalogTestScenario,
    TracerCatalog,
};
pub use command_bus::CommandBusTransport;
pub use correlation::{
    CorrelationCommandOutcome, CorrelationError, CorrelationEvent, CorrelationEventKind,
    CorrelationObserver, CorrelationSubscription, DomainEventObservation,
    IntegrationEventObservation,
};
pub use input::{CommandInputDocument, CommandInputField, CommandInputOption, CommandInputOptions};
pub use message_series::{
    ExpectedMessageNode, MessageGraphDefinition, MessageSeriesDefinition,
    MessageSeriesDefinitionError, MessageSeriesValidationIssue, ObservedCommandOutcome,
    ObservedMessageNode, ObservedMessageSeries, ObservedMessageSeriesError,
    ObservedMessageSeriesOutcomeIssue, message_series_definition_schema,
    observed_message_series_schema,
};
pub use operation::{
    CompletedDecision, OperationEvent, OperationEventKind, OperationMode, OperationResult,
    OperationSnapshot, OperationStatus, OperationSubscription, PredictedDomainEvent,
    SubscriptionError,
};
pub use runtime::RuntimeRegistrationError;
pub use service::{
    CommandInputError, DiscoveryError, ExposeTracePayloadsForLocalDevelopment,
    MAX_COMMAND_PAYLOAD_LEN, RedactTracePayloads, SimulationRequest, SubmissionError, TestRunError,
    TestScenarioReset, TestScenarioResetError, TracePayloadPolicy, Tracer, TracerBuilder,
};
pub use transport::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandReceipt, CommandRejection,
    CommandTransport, CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
    command_execution_fingerprint,
};
