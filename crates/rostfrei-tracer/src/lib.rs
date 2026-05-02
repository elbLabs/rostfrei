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

pub use behavioral::behavioral_test_definition_schema as behavioral_test_schema;
pub use behavioral::{
    FilesystemTestRepository, TestAggregate, TestCommand, TestDefinition, TestDefinitionCollection,
    TestDefinitionError, TestDefinitionRevision, TestDefinitionSummary,
    TestDefinitionValidationIssue, TestOutcome, TestRejection, TestReport, TestReportStatus,
    TestRepository, TestRepositoryError, TestSetup, TestTimeout, TestTimeoutParseError, TestTrace,
    TraceExpectation, behavioral_test_definition_schema, outcome_matches, payload_matches_subset,
    trace_expectation_matches,
};
pub use catalog::{
    AggregateInstanceCollection, AggregateInstanceSummary, CatalogAggregate, CatalogBehavioralTest,
    CatalogCommand, CatalogCommandVersion, CatalogContext, CatalogTestRepository,
    CatalogTestScenario, TestFixtureCollection, TestFixtureSummary, TracerCatalog,
};
pub use command_bus::CommandBusTransport;
pub use correlation::{
    CorrelationCommandOutcome, CorrelationError, CorrelationEvent, CorrelationEventKind,
    CorrelationObserver, CorrelationSubscription, DomainEventObservation,
    IntegrationEventObservation,
};
pub use input::{CommandInputDocument, CommandInputField, CommandInputOption, CommandInputOptions};
pub use message_series::{
    ExpectedCommandFields, ExpectedMessageKind, ExpectedMessageNode, MessageGraphDefinition,
    MessageSeriesComparison, MessageSeriesComparisonContext, MessageSeriesComparisonDiagnostic,
    MessageSeriesComparisonStatus, MessageSeriesDefinition, MessageSeriesDefinitionError,
    MessageSeriesMatch, MessageSeriesValidationIssue, ObservedCommandOutcome, ObservedMessageNode,
    ObservedMessageSeries, ObservedMessageSeriesError, ObservedMessageSeriesOutcomeIssue,
    compare_message_series, message_series_definition_schema, observed_message_series_schema,
};
pub use operation::{
    CompletedDecision, OperationEvent, OperationEventKind, OperationMode, OperationResult,
    OperationSnapshot, OperationStatus, OperationSubscription, PredictedDomainEvent,
    SubscriptionError,
};
pub use rostfrei_fixtures::{
    Fixture, FixtureAggregate, FixtureApplyError, FixtureApplyReport,
    FixtureCodecRegistrationError, FixtureDomainEvent, FixtureValidationError, MessageSeriesEngine,
};
pub use runtime::RuntimeRegistrationError;
pub use service::{
    CommandInputError, DiscoveryError, ExposeTracePayloadsForLocalDevelopment,
    MAX_COMMAND_PAYLOAD_LEN, MessageSeriesCapture, MessageSeriesCaptureError,
    MessageSeriesFidelity, OperationMessageSeries, RedactTracePayloads, SimulationRequest,
    SubmissionError, TestDefinitionValidationError, TestRunError, TestScenarioReset,
    TestScenarioResetError, TracePayloadPolicy, Tracer, TracerBuilder,
};
pub use transport::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandReceipt, CommandRejection,
    CommandTransport, CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
    command_execution_fingerprint,
};
