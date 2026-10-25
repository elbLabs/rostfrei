mod behavioral;
mod bindings;
mod catalog;
mod command_bus;
mod correlation;
mod correlation_view;
mod handler_simulation;
mod input;
mod message_series;
mod observation;
mod operation;
mod quarantine;
mod runtime;
mod service;
pub mod transport;

#[cfg(feature = "nats-host")]
mod nats_host;
pub use bindings::{ApplicationBindingError, ApplicationBindings};
#[cfg(feature = "nats-host")]
pub use nats_host::{
    NatsApplication, NatsApplicationBuilder, NatsExecutionProfile, NatsHostError, NatsRuntime,
    NatsRuntimeConfig, TracerConfig,
};

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
    AggregateInstanceCollection, AggregateInstanceSummary, CatalogAggregate, CatalogAnchor,
    CatalogBehavioralTest, CatalogCommand, CatalogCommandVersion, CatalogContext,
    CatalogCorrelations, CatalogQuarantine, CatalogQuarantineScope, CatalogTestRepository,
    CatalogTestScenario, TestFixtureCollection, TestFixtureSummary, TracerCatalog,
};
pub use command_bus::CommandBusTransport;
pub use correlation::{
    CorrelationCommandOutcome, CorrelationError, CorrelationEvent, CorrelationEventKind,
    CorrelationObserver, CorrelationSubscription, DomainEventObservation,
    IntegrationEventObservation,
};
pub use correlation_view::{
    AnchorCorrelations, AnchorIdentity, AnchoredCorrelation, CorrelationInspectionError,
    CorrelationRefView, DomainRefView, IndexedCorrelation, IntegrationRefView,
    MAX_ANCHOR_CORRELATIONS, MAX_INCLUDED_CONTEXTS, MAX_INCLUDED_STREAMS,
};
pub use handler_simulation::{
    HandlerDecision, HandlerSimulationError, HandlerSimulationKind, HandlerSimulationResult,
    ProposedDomainEvent,
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
pub use observation::{
    CatalogObservation, ObservationError, ObservationFeed, ObservationScope, ObservationSnapshot,
    ObservationSource, ObservationStatus, ObservationSubscription, ObservedFlow,
    ObservedFlowSummary,
};
pub use operation::{
    CompletedDecision, OperationEvent, OperationEventEvidence, OperationEventEvidenceKind,
    OperationEventKind, OperationMode, OperationResult, OperationSnapshot, OperationStatus,
    OperationSubscription, PredictedDomainEvent, SubscriptionError, TouchedStreamParticipant,
};
pub use quarantine::{
    DefaultQuarantinePayloadPolicy, QuarantineCollection, QuarantineDetail,
    QuarantineInspectionError, QuarantinePayload, QuarantinePayloadContent,
    QuarantinePayloadPolicy, QuarantinePayloadStatus, QuarantineScope, QuarantineSource,
    QuarantineSummary,
};
pub use rostfrei_fixtures::{
    Fixture, FixtureAggregate, FixtureApplyError, FixtureApplyReport,
    FixtureCodecRegistrationError, FixtureDomainEvent, FixtureValidationError, MessageSeriesEngine,
};
pub use runtime::RuntimeRegistrationError;
pub use service::{
    CommandInputError, CommandRequest, DiscoveryError, ExposeTracePayloadsForLocalDevelopment,
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
