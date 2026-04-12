extern crate self as rostfrei;

pub use domain::*;
pub use rostfrei_core::{
    Aggregate, AggregateId as StreamAggregateId, AggregateType as StreamAggregateType,
    AppendOutcome, CommandHandler, CommittedDomainEvent, ContentFingerprint, DecisionContext,
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, DomainEventRegistrationError, EventBatch, EventCodec,
    EventCodecError, EventCodecErrorKind, EventId, EventStore, EventStoreError,
    EventStoreErrorKind, EventVariant, ExecutionError, ExecutionMetadata, ExecutionOutcome,
    Executor, ExpectedVersion, InMemoryEventStore, NewEvent, OperationId, RecordedEvent, StreamId,
    StreamVersion,
};
pub use rostfrei_domain_runtime::{domain_module, AggregateRuntime, Apply, Initialize};
pub use rostfrei_registry::{
    CommandDefinition, CommandDescriptor, DomainModule, DomainRegistry, ModuleDescriptor,
    RegistrationError,
};

#[doc(hidden)]
pub mod __private {
    pub use domain::__private::{AggregateActionOutput, DomainServiceActionOutput};
    pub use rostfrei_domain_runtime as domain_runtime;
}
