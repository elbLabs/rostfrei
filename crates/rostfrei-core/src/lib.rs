mod aggregate;
mod domain_event;
mod envelope;
mod executor;
mod identity;
mod memory;
mod store;

pub use aggregate::{
    Aggregate, CommandHandler, DecisionContext, Event, EventCodec, EventCodecError,
    EventCodecErrorKind, EventVariant, JsonEventCodec,
};
pub use domain_event::{
    CommittedDomainEvent, DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler,
    DomainEventHandlerError, DomainEventHandlerErrorKind, DomainEventRegistrationError,
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

#[doc(hidden)]
pub mod __private {
    use serde::{de::DeserializeOwned, Serialize};

    use crate::{EventCodecError, EventCodecErrorKind};

    pub fn encode_json<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, EventCodecError> {
        serde_json::to_vec(value).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
        })
    }

    pub fn decode_json<T: DeserializeOwned>(payload: &[u8]) -> Result<T, EventCodecError> {
        serde_json::from_slice(payload).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
        })
    }
}
