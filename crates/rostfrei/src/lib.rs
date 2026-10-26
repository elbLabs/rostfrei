//! Domain modeling, event sourcing, and typed messaging for Rust applications.
//!
//! Rostfrei connects business rules written in ordinary Rust to a compiled domain
//! model, event-store execution, and typed command, query, and integration-event
//! buses. Aggregates stay independent of persistence, serialization, and brokers.
//!
//! Explicit ownership, typed contracts, and inspectable command flows help humans
//! and coding agents understand the same codebase, including agent-written code.
//!
//! Start with the [getting-started guide](https://elblabs.github.io/rostfrei/docs/getting-started)
//! and the [bike-rental example](https://github.com/elbLabs/rostfrei/tree/main/examples/bike-rental).
//! The [documentation](https://elblabs.github.io/rostfrei/docs) covers domain
//! declarations, project structure, and NATS messaging. Rostfrei is currently
//! alpha and its APIs are evolving.

extern crate self as rostfrei;

#[doc(hidden)]
#[macro_export]
macro_rules! __rostfrei_macro_support_runtime {
    ($($tokens:tt)*) => {
        $($tokens)*
    };
}

/// Installs the crate-local support bridge used by Rostfrei procedural macros.
///
/// Invoke this exactly once in the root of every crate that declares Rostfrei
/// domain types or derives an application `QueryDefinition`.
#[macro_export]
macro_rules! install_macro_support {
    () => {
        #[doc(hidden)]
        pub mod __rostfrei_macro_support {
            pub use $crate::__rostfrei_macro_support_runtime as __runtime;
            pub use $crate::*;

            pub mod __private {
                pub use $crate::__private::domain_runtime::__private::{
                    assert_unique_event_ids, core,
                };
                pub use $crate::__private::*;
            }
        }
    };
}

mod command_bus;
mod command_rejection_codec;
mod command_service;
pub use command_bus::processing::CommandProcessorBuilder;
pub use command_service::{
    AllowAllCommands, AttemptId, AuditDelivery, AuditError, AuditPhase, AuditTerminal,
    AuthorizationError, CommandAccess, CommandAuditRecord, CommandAuditSink, CommandAuthorizer,
    CommandProcessingService, CompletionOrigin, InMemoryCommandAuditSink, PrincipalId,
    ProcessedCommand, ServiceReport, TrustedCommandContext, ValidatedCommand,
};
mod in_memory_messaging;
mod integration_event;
mod integration_event_bus;
mod query_bus;
mod read_model;
pub use command_rejection_codec::MappedCommandRejectionCodec;

pub use command_bus::{
    CommandBindingRegistrationError, CommandBus, CommandBusError, CommandBusErrorKind,
    CommandBusObserver, CommandBusReceipt, CommandMessageAdapter, CommandProcessor,
    CommandProcessorError, CommandProcessorErrorKind, CommandPublication, CommandRejectionMapper,
    CommandRequest, DynamicCommandRequest, EncodedCommand, InfallibleCommandRejectionMapper,
    InvalidCommandResponse, JsonDomainRejectionMapper, RoutedCommand, RoutedCommandError,
    command_execution_fingerprint, command_message_id, command_response_message_id,
    command_technical_response_message_id,
};
pub use domain::*;
pub use in_memory_messaging::InMemoryMessagingAdapter;
pub use integration_event::{
    CompletedIntegrationCommand, IntegrationCommandMapper, IntegrationEventCommandHandler,
    IntegrationEventProcessingError, IntegrationEventProcessor,
};
pub use integration_event_bus::{
    CommittedEventContext, EncodedIntegrationMessage, IntegrationEvent, IntegrationEventBus,
    IntegrationEventBusError, IntegrationEventBusErrorKind, IntegrationEventDispatcherExt,
    IntegrationEventMapper, IntegrationEventPublication, IntegrationMessageAdapter,
    integration_message_id,
};
pub use query_bus::{
    DynamicQueryRequest, EncodedQuery, InMemoryQueryAdapter, QueryBindingRegistrationError,
    QueryBus, QueryBusError, QueryBusErrorKind, QueryMessageAdapter, QueryProcessor,
    QueryProcessorHandler, QueryRequest, RoutedQuery, RoutedQueryError,
};
pub use read_model::{
    IntegrationEventOrder, NoDomainSource, ReadModelBackend, ReadModelBuilder,
    ReadModelDomainEvent, ReadModelIntegrationBinding, ReadModelProcessingError, ReadModelReader,
    ReadModelRuntime, ReadModelState, ReadModels,
};
pub use rostfrei_core::{
    ActorRef, Aggregate, AggregateCodecs, AggregateId as StreamAggregateId, AggregateInstance,
    AggregateType as StreamAggregateType, AppendOutcome, AppendSession, BoundedConflictRetries,
    CommandAuthority, CommandCommitSession, CommandCompletion, CommandDecision, CommandExecution,
    CommandExecutionError, CommandExecutionMetadata, CommandExecutor, CommandHandler,
    CommandHandlingResult, CommandOutcome, CommandReceipt, CommandResult, CommittedDomainEvent,
    CompletedAppendOutcome, CompletedCommit, CompletedOutcome, ConflictRetry,
    ConflictRetryDecision, ConflictRetryPolicy, ContentFingerprint, CorrelationChain,
    CorrelationIndexEntry, CorrelationIndexFreshness, CorrelationIndexLag, CorrelationIndexReader,
    CorrelationRef, CorrelationSourceFreshness, CorrelationSourceKind, CorrelationSourceState,
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, DomainEventRef, DomainEventRegistrationError,
    EncodedOutcomeDetails, EncodedRejection, EventBatch, EventCodec, EventCodecError,
    EventCodecErrorKind, EventHistory, EventId, EventStore, EventStoreError, EventStoreErrorKind,
    EventStoreErrorOrigin, EventTransaction, EventVariant, ExecutionStore, ExpectedVersion,
    InMemoryEventStore, InlineCompletedOutcome, IntegrationEventRef, JsonReadModelCodec,
    JsonRejectionCodec, LoadedAggregate, MAX_ACTOR_ID_BYTES, MAX_ACTOR_NAMESPACE_BYTES,
    MAX_CORRELATION_REFS, MAX_REJECTION_BYTES, MAX_TRANSACTION_ITEMS, NewEvent, OperationId,
    OutcomeDetails, OutcomeEvidence, OutcomeExpiry, OutcomeKey, OutcomeLifecycle, OutcomeLookup,
    OutcomeReader, OutcomeRecoveryError, OutcomeRevision, ReadModel, ReadModelCodec,
    ReadModelEntry, ReadModelError, ReadModelErrorKind, ReadModelKey, ReadModelRevision,
    ReadModelStore, ReclamationStatus, RecordedEvent, RejectionCodec, SecurityScope,
    SimulatedParticipant, SimulationError, SimulationOutcome, StreamId, StreamVersion,
    TransactionAppendOutcome, TransactionParticipant, TransactionReceipt, TransactionStreamReceipt,
};
pub use rostfrei_domain_runtime::{AggregateEventRuntime, AggregateRuntime, Apply, Initialize};
pub use rostfrei_macros::{QueryDefinition, ReadModel};
pub use rostfrei_messaging_core::{
    ApplicationErrorCode, ApplicationName, BoundedContext, BoundedContextName, CallerMetadata,
    CausationId, CodecFailureKind, CommandAddress, CommandEnvelope, CommandPublisher,
    CommandRejection, CommandRejectionClassification, CommandResponse, CommandResponseOutcome,
    CommandResponseReader, CorrelationId, DurableName, ExecutionFailure, ExecutionRecovery,
    IntegrationEventEnvelope, MessageId, MessageTimestamp, ProcessorFailureKind, QueryAddress,
    QueryErrorClassification, QueryErrorPayload, QueryHandler, QueryOptions, QueryOutcome,
    QueryRequest as QueryHandlerRequest, QueryRequestError, QueryRequestErrorKind, QueryResponse,
    QueryResult, QueryServer, StoreFailureKind, TraceContext, TrafficScope,
};
pub use rostfrei_registry::{
    CommandDefinition, CommandDescriptor as CommandRegistrationDescriptor, DomainRegistry,
    QueryDefinition, QueryDescriptor as QueryRegistrationDescriptor, RegistrationError,
};

#[doc(hidden)]
pub mod __private {
    pub use domain::__private::{emit_domain_test_descriptor, serde, serde_json};
    pub use rostfrei_domain_runtime as domain_runtime;
    pub use rostfrei_registry as registry;
}
