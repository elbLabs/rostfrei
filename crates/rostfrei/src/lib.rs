extern crate self as rostfrei;

mod integration_event;

pub use domain::*;
pub use integration_event::{
    CommandContext, CommandRejection, IntegrationEventHandler, IntegrationEventOutcome,
    IntegrationEventProcessingError, IntegrationEventProcessor,
};
pub use rostfrei_core::{
    Aggregate, AggregateId as StreamAggregateId, AggregateInstance,
    AggregateType as StreamAggregateType, AppendOutcome, CommandExecutionError, CommandHandler,
    CommandOutcome, CommandReceipt, CommandResult, CommittedDomainEvent, ContentFingerprint,
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, DomainEventRegistrationError, EventBatch, EventCodec,
    EventCodecError, EventCodecErrorKind, EventHistory, EventId, EventStore, EventStoreError,
    EventStoreErrorKind, EventVariant, ExecutionMetadata, Executor, ExpectedVersion,
    InMemoryEventStore, NewEvent, OperationId, RecordedEvent, SimulationDecision, SimulationError,
    SimulationOutcome, StreamId, StreamVersion,
};
pub use rostfrei_domain_runtime::{
    AggregateRuntime, Apply, Initialize, domain_command_handler, domain_module,
};
pub use rostfrei_messaging_core::{DurableName, IntegrationEventEnvelope};
pub use rostfrei_registry::{
    CommandDefinition, CommandDescriptor, DomainModule, DomainRegistry, ModuleDescriptor,
    RegistrationError,
};

#[doc(hidden)]
pub mod __private {
    pub use domain::__private::{
        AggregateActionOutput, DomainServiceActionOutput, EntityActionOutput, SameType,
        ValueObjectActionOutput, emit_domain_test_descriptor, serde, serde_json,
    };
    pub use rostfrei_domain_runtime as domain_runtime;
}
