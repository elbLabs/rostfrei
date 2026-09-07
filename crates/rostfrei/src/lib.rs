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
mod in_memory_messaging;
mod integration_event;
mod integration_event_bus;
mod query_bus;

pub use command_bus::{
    CommandBindingRegistrationError, CommandBus, CommandBusError, CommandBusErrorKind,
    CommandBusObserver, CommandBusReceipt, CommandMessageAdapter, CommandProcessor,
    CommandProcessorError, CommandProcessorErrorKind, CommandPublication, CommandRejectionMapper,
    CommandRequest, DynamicCommandRequest, EncodedCommand, InfallibleCommandRejectionMapper,
    InvalidCommandResponse, JsonDomainRejectionMapper, RoutedAggregateCommand,
    RoutedAggregateCommandError, command_execution_fingerprint, command_message_id,
    command_response_message_id,
};
pub use domain::*;
pub use in_memory_messaging::InMemoryMessagingAdapter;
pub use integration_event::{
    CompletedIntegrationCommand, IntegrationCommand, IntegrationCommandMapper,
    IntegrationEventCommandHandler, IntegrationEventProcessingError, IntegrationEventProcessor,
};
pub use integration_event_bus::{
    CommittedEventContext, EncodedIntegrationMessage, IntegrationEvent, IntegrationEventBus,
    IntegrationEventBusError, IntegrationEventBusErrorKind, IntegrationEventDispatcherExt,
    IntegrationEventMapper, IntegrationEventPublication, IntegrationEventPublisher,
    IntegrationMessageAdapter, integration_message_id,
};
pub use query_bus::{
    DynamicQueryRequest, EncodedQuery, InMemoryQueryAdapter, QueryBindingRegistrationError,
    QueryBus, QueryBusError, QueryBusErrorKind, QueryMessageAdapter, QueryProcessor,
    QueryProcessorHandler, QueryRequest, RoutedQuery, RoutedQueryError,
};
pub use rostfrei_core::{
    Aggregate, AggregateId as StreamAggregateId, AggregateInstance,
    AggregateType as StreamAggregateType, AppendOutcome, CommandExecutionError, CommandHandler,
    CommandOutcome, CommandReceipt, CommandResult, CommittedDomainEvent, ContentFingerprint,
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, DomainEventRegistrationError, EventBatch, EventCodec,
    EventCodecError, EventCodecErrorKind, EventHistory, EventId, EventStore, EventStoreError,
    EventStoreErrorKind, EventTransaction, EventVariant, ExecutionMetadata, Executor,
    ExpectedVersion, InMemoryEventStore, MAX_TRANSACTION_ITEMS, NewEvent, OperationId,
    RecordedEvent, SimulationDecision, SimulationError, SimulationOutcome, StreamId, StreamVersion,
    TransactionAppendOutcome, TransactionParticipant, TransactionReceipt, TransactionStreamReceipt,
};
pub use rostfrei_domain_runtime::{AggregateEventRuntime, AggregateRuntime, Apply, Initialize};
pub use rostfrei_macros::QueryDefinition;
pub use rostfrei_messaging_core::{
    ApplicationErrorCode, ApplicationName, BoundedContext, BoundedContextName, CallerMetadata,
    CausationId, CommandAddress, CommandPublisher, CommandRejection,
    CommandRejectionClassification, CommandResponse, CommandResponseOutcome, CommandResponseReader,
    CorrelationId, DurableName, IntegrationEventEnvelope, MessageId, MessageTimestamp,
    QueryAddress, QueryErrorClassification, QueryErrorPayload, QueryHandler, QueryOptions,
    QueryOutcome, QueryRequest as QueryHandlerRequest, QueryRequestError, QueryRequestErrorKind,
    QueryResponse, QueryResult, QueryServer, TraceContext, TrafficScope,
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
