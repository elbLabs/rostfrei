extern crate self as rostfrei;

mod command_bus;
mod in_memory_messaging;
mod integration_event;
mod integration_event_bus;

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
    CommandContext, CompletedIntegrationCommand, IntegrationEventHandler, IntegrationEventOutcome,
    IntegrationEventProcessingError, IntegrationEventProcessor,
};
pub use integration_event_bus::{
    CommittedEventContext, EncodedIntegrationMessage, IntegrationEvent, IntegrationEventBus,
    IntegrationEventBusError, IntegrationEventBusErrorKind, IntegrationEventPublication,
    IntegrationMessageAdapter, integration_message_id,
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
pub use rostfrei_domain_runtime::{AggregateRuntime, Apply, Initialize, domain_module};
pub use rostfrei_messaging_core::{
    CausationId, CommandAddress, CommandPublisher, CommandRejection,
    CommandRejectionClassification, CommandResponse, CommandResponseOutcome, CommandResponseReader,
    CorrelationId, DurableName, IntegrationEventEnvelope, MessageId, MessageTimestamp,
};
pub use rostfrei_registry::{
    CommandDefinition, CommandDescriptor as CommandRegistrationDescriptor, DomainModule,
    DomainRegistry, ModuleDescriptor, RegistrationError,
};

#[doc(hidden)]
pub mod __private {
    pub use domain::__private::{
        AggregateActionOutput, AttachedDecisionGroup, DomainServiceActionOutput,
        EntityActionOutput, SameType, ValueObjectActionOutput, emit_domain_test_descriptor, serde,
        serde_json,
    };
    pub use rostfrei_domain_runtime as domain_runtime;
}
