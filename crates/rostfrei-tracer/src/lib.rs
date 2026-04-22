mod behavioral;
mod catalog;
mod correlation;
mod input;
mod operation;
mod runtime;
mod service;
pub mod transport;

#[cfg(feature = "http")]
pub mod http;

pub use behavioral::{
    outcome_matches, payload_matches_subset, trace_expectation_matches, FilesystemTestRepository,
    TestAggregate, TestCommand, TestDefinition, TestDefinitionCollection, TestDefinitionRevision,
    TestDefinitionSummary, TestExpectationResult, TestGiven, TestOutcome, TestRejection,
    TestReport, TestReportFailure, TestReportStatus, TestRepository, TestRepositoryError, TestThen,
    TestTimeout, TestTimeoutParseError, TestTrace, TestWhen, TraceExpectation,
};
pub use catalog::{
    AggregateInstanceCollection, AggregateInstanceSummary, CatalogAggregate, CatalogCommand,
    CatalogCommandVersion, CatalogContext, CatalogTestRepository, CatalogTestScenario,
    TracerCatalog,
};
pub use correlation::{
    CorrelationCommandOutcome, CorrelationError, CorrelationEvent, CorrelationEventKind,
    CorrelationObserver, CorrelationSubscription, DomainEventObservation,
    IntegrationEventObservation,
};
pub use input::{CommandInputDocument, CommandInputField, CommandInputOption, CommandInputOptions};
pub use operation::{
    CompletedDecision, OperationEvent, OperationEventKind, OperationMode, OperationResult,
    OperationSnapshot, OperationStatus, OperationSubscription, PredictedDomainEvent,
    SubscriptionError,
};
pub use runtime::RuntimeRegistrationError;
pub use service::{
    CommandInputError, DiscoveryError, ExposeTracePayloadsForLocalDevelopment, RedactTracePayloads,
    SimulationRequest, SubmissionError, TestRunError, TestScenarioReset, TestScenarioResetError,
    TracePayloadPolicy, Tracer, TracerBuilder, MAX_COMMAND_PAYLOAD_LEN,
};
pub use transport::{
    command_execution_fingerprint, CommandInvocation, CommandOutcome, CommandPublication,
    CommandReceipt, CommandRejection, CommandTransport, CommandTransportError,
    CommandTransportErrorKind, CommandTransportObserver,
};
