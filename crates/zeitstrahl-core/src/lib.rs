mod aggregate;
mod envelope;
mod executor;
mod identity;
mod memory;
mod store;

pub use aggregate::{
    Aggregate, CommandHandler, DecisionContext, EventCodec, EventCodecError, EventCodecErrorKind,
};
pub use envelope::{
    EnvelopeError, EventBatch, ExpectedVersion, NewEvent, RecordedEvent, StreamVersion,
    MAX_BATCH_PAYLOAD_LEN, MAX_EVENTS_PER_BATCH, MAX_EVENT_PAYLOAD_LEN, MAX_EVENT_TYPE_LEN,
};
pub use executor::{ExecutionError, ExecutionOutcome, Executor};
pub use identity::{
    AggregateId, AggregateType, CommitId, ContentFingerprint, EventId, ExecutionMetadata,
    IdentityError, OperationId, StreamId,
};
pub use memory::InMemoryEventStore;
pub use store::{AppendOutcome, EventStore, EventStoreError, EventStoreErrorKind};
