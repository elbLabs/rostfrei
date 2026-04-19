extern crate self as rostfrei;

mod integration_event;

pub use domain::*;
pub use integration_event::{
    CommandContext, CompletedIntegrationCommand, IntegrationEventHandler, IntegrationEventOutcome,
    IntegrationEventProcessingError, IntegrationEventProcessor, InvalidCommandResponse,
    RoutedAggregateCommand, RoutedAggregateCommandError,
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
    CommandAddress, CommandPublisher, CommandRejection, CommandRejectionClassification,
    CommandResponse, CommandResponseOutcome, CommandResponseReader, DurableName,
    IntegrationEventEnvelope,
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
