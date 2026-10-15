mod aggregate;
mod append_session;
mod codecs;
mod command_backend;
mod completed_outcome;
mod conflict_retry;
mod domain_event;
mod envelope;
mod executor;
mod identity;
mod memory;
mod outcome;
mod proposal;
mod read_model;
mod receipt_validation;
mod store;

pub use aggregate::{
    Aggregate, AggregateInstance, Event, EventCodec, EventCodecError, EventCodecErrorKind,
    EventVariant, JsonEventCodec,
};
pub use append_session::AppendSession;
pub use codecs::{AggregateCodecs, CodecRegistrationError, CodecValidationError};
pub use command_backend::*;
pub use completed_outcome::{
    CommandCompletion, CompletedAppendOutcome, EncodedRejection, InlineCompletedOutcome,
    JsonRejectionCodec, MAX_REJECTION_BYTES, OutcomeEvidence, RejectionCodec,
    validate_completed_transaction,
};
pub use conflict_retry::*;
pub use domain_event::{
    CommittedDomainEvent, DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler,
    DomainEventHandlerError, DomainEventHandlerErrorKind, DomainEventRegistrationError,
};
pub use envelope::{
    EnvelopeError, EventBatch, ExpectedVersion, MAX_BATCH_PAYLOAD_LEN, MAX_EVENT_PAYLOAD_LEN,
    MAX_EVENT_TYPE_LEN, MAX_EVENTS_PER_BATCH, NewEvent, RecordedEvent, StreamVersion,
};
pub use executor::{
    CommandDecision, CommandExecution, CommandExecutionError, CommandExecutor, CommandHandler,
    CommandHandlingResult, CommandOutcome, CommandReceipt, CommandResult, LoadedAggregate,
    SimulatedParticipant, SimulationError, SimulationOutcome,
};
pub use identity::{
    AggregateId, AggregateType, CommandExecutionMetadata, CommitId, ContentFingerprint, EventId,
    IdentityError, OperationId, SecurityScope, StreamId, derive_commit_id, derive_event_id,
};
pub use memory::InMemoryEventStore;
pub use outcome::{
    ArchiveReference, CompletedOutcome, DetailRead, DetailReadError, DetailUnavailableReason,
    EncodedOutcomeDetails, OutcomeDecision, OutcomeDetailReader, OutcomeDetailReference,
    OutcomeDetails, OutcomeMarker, OutcomeRecoveryError, resolve_details,
};
pub use proposal::ProposalDigest;
pub use read_model::{
    JsonReadModelCodec, ReadModel, ReadModelCodec, ReadModelEntry, ReadModelError,
    ReadModelErrorKind, ReadModelKey, ReadModelRevision, ReadModelStore,
};
pub use store::{
    AppendOutcome, EventHistory, EventStore, EventStoreError, EventStoreErrorKind,
    EventStoreErrorOrigin, EventTransaction, MAX_TRANSACTION_ITEMS, StreamDirectory, StreamSummary,
    TransactionAppendOutcome, TransactionParticipant, TransactionReceipt, TransactionStreamReceipt,
    validate_transaction_item_limit,
};

#[doc(hidden)]
pub mod __private {
    use serde::{Serialize, de::DeserializeOwned};

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
