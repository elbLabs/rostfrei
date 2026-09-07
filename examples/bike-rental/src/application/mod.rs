use rostfrei::{Command, JsonCommandPayload, command_execution_fingerprint};
use rostfrei_core::{
    Aggregate, AggregateInstance, CommandExecutionError, EventBatch, EventCodec, EventStore,
    EventStoreError, EventStoreErrorKind, EventTransaction, ExecutionMetadata, ExpectedVersion,
    JsonEventCodec, RecordedEvent, StreamId, StreamVersion, TransactionAppendOutcome,
    TransactionParticipant, TransactionReceipt,
};

use crate::domain::rental_fleet::{
    self, BicycleTransfer, BicycleTransferRejected, RentalFleetAggregate, TransferBicycle,
};

const DEFAULT_MAX_CONFLICT_RETRIES: usize = 3;

/// Application-level outcome of transferring a bicycle between rental fleets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferBicycleOutcome {
    Accepted(TransactionReceipt),
    ExactReplay(TransactionReceipt),
    Rejected(BicycleTransferRejected),
}

impl TransferBicycleOutcome {
    pub const fn receipt(&self) -> Option<&TransactionReceipt> {
        match self {
            Self::Accepted(receipt) | Self::ExactReplay(receipt) => Some(receipt),
            Self::Rejected(_) => None,
        }
    }

    pub const fn is_exact_replay(&self) -> bool {
        matches!(self, Self::ExactReplay(_))
    }
}

/// Coordinates the atomic transfer of a bicycle between two rental-fleet streams.
pub struct TransferBicycleHandler<S> {
    store: S,
    maximum_conflict_retries: usize,
}

impl<S> TransferBicycleHandler<S> {
    pub const fn new(store: S) -> Self {
        Self {
            store,
            maximum_conflict_retries: DEFAULT_MAX_CONFLICT_RETRIES,
        }
    }

    #[must_use]
    pub const fn with_max_conflict_retries(mut self, maximum_conflict_retries: usize) -> Self {
        self.maximum_conflict_retries = maximum_conflict_retries;
        self
    }

    pub const fn store(&self) -> &S {
        &self.store
    }
}

impl<S> TransferBicycleHandler<S>
where
    S: EventStore,
{
    pub async fn handle(
        &self,
        source_metadata: ExecutionMetadata,
        command: &TransferBicycle,
    ) -> Result<TransferBicycleOutcome, CommandExecutionError> {
        validate_source_stream(source_metadata.stream_id())?;
        validate_operation_fingerprint(&source_metadata, command)?;
        let destination_stream =
            rental_fleet::stream_id(command.to_fleet_id.as_str()).map_err(invalid_request)?;
        let destination_metadata =
            metadata_for_stream(&source_metadata, destination_stream.clone());

        let mut remaining_conflict_retries = self.maximum_conflict_retries;
        loop {
            if let Some(outcome) = self
                .reconcile_receipt(&source_metadata, destination_metadata.stream_id())
                .await?
            {
                return Ok(outcome);
            }

            let source_history = self.store.load(source_metadata.stream_id()).await?;
            let destination_history = self.store.load(destination_metadata.stream_id()).await?;
            let source_version = current_version(&source_history);
            let destination_version = current_version(&destination_history);

            let mut source = rehydrate(source_metadata.stream_id().clone(), &source_history)?;
            let mut destination = rehydrate(destination_stream.clone(), &destination_history)?;
            if let Err(rejection) =
                BicycleTransfer::transfer(&mut source, &mut destination, command)
            {
                if let Some(outcome) = self
                    .reconcile_receipt(&source_metadata, destination_metadata.stream_id())
                    .await?
                {
                    return Ok(outcome);
                }
                return Ok(TransferBicycleOutcome::Rejected(rejection));
            }

            let source_batch = encode_batch(&source_metadata, source.uncommitted_events())?;
            let destination_batch =
                encode_batch(&destination_metadata, destination.uncommitted_events())?;
            let transaction = transaction(
                &source_metadata,
                destination_metadata.stream_id(),
                source_version,
                source_batch,
                destination_version,
                destination_batch,
            );

            match self.store.append_transaction(transaction).await {
                Ok(TransactionAppendOutcome::Appended(receipt)) => {
                    return Ok(TransferBicycleOutcome::Accepted(receipt));
                }
                Ok(TransactionAppendOutcome::ExactReplay(receipt)) => {
                    return Ok(TransferBicycleOutcome::ExactReplay(receipt));
                }
                Err(error)
                    if error.kind() == EventStoreErrorKind::Conflict
                        && remaining_conflict_retries > 0 =>
                {
                    remaining_conflict_retries = remaining_conflict_retries.saturating_sub(1);
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn reconcile_receipt(
        &self,
        source_metadata: &ExecutionMetadata,
        destination_stream: &StreamId,
    ) -> Result<Option<TransferBicycleOutcome>, CommandExecutionError> {
        self.store
            .load_transaction_receipt(source_metadata.stream_id(), source_metadata.operation_id())
            .await?
            .map(|receipt| replay_outcome(receipt, source_metadata, destination_stream))
            .transpose()
    }
}

fn replay_outcome(
    receipt: TransactionReceipt,
    source_metadata: &ExecutionMetadata,
    destination_stream: &StreamId,
) -> Result<TransferBicycleOutcome, CommandExecutionError> {
    let streams = receipt.streams();
    let exact = receipt.operation_id() == source_metadata.operation_id()
        && receipt.operation_fingerprint() == source_metadata.operation_fingerprint()
        && receipt.correlation_id() == source_metadata.correlation_id()
        && receipt.causation_id() == source_metadata.causation_id()
        && streams.len() == 2
        && streams
            .first()
            .is_some_and(|stream| stream.stream_id() == source_metadata.stream_id())
        && streams
            .get(1)
            .is_some_and(|stream| stream.stream_id() == destination_stream);
    if exact {
        Ok(TransferBicycleOutcome::ExactReplay(receipt))
    } else {
        Err(EventStoreError::new(
            EventStoreErrorKind::IdentityConflict,
            "transaction operation identity was reused with different command content or context",
        )
        .into())
    }
}

fn validate_operation_fingerprint(
    metadata: &ExecutionMetadata,
    command: &TransferBicycle,
) -> Result<(), EventStoreError> {
    let payload = command
        .encode_json()
        .map_err(|error| invalid_request(format!("failed to encode transfer command: {error}")))?;
    let expected = command_execution_fingerprint(
        metadata.stream_id().aggregate_type().as_str(),
        metadata.stream_id().aggregate_id().as_str(),
        TransferBicycle::LOCAL_ID,
        TransferBicycle::SCHEMA_VERSION,
        &payload,
    )
    .map_err(|error| invalid_request(format!("failed to fingerprint transfer command: {error}")))?;
    if metadata.operation_fingerprint() == expected {
        Ok(())
    } else {
        Err(invalid_request(
            "transfer command execution fingerprint does not match its content and source stream",
        ))
    }
}

fn metadata_for_stream(source: &ExecutionMetadata, stream_id: StreamId) -> ExecutionMetadata {
    let mut metadata = ExecutionMetadata::new(
        stream_id,
        source.operation_id().clone(),
        source.operation_fingerprint(),
    );
    if let Some(correlation_id) = source.correlation_id() {
        metadata = metadata.with_correlation_id(correlation_id.clone());
    }
    if let Some(causation_id) = source.causation_id() {
        metadata = metadata.with_causation_id(causation_id.clone());
    }
    metadata
}

fn rehydrate(
    stream_id: StreamId,
    history: &[RecordedEvent],
) -> Result<AggregateInstance<RentalFleetAggregate>, CommandExecutionError> {
    let events = history
        .iter()
        .map(|event| {
            <JsonEventCodec as EventCodec<RentalFleetAggregate>>::decode(&JsonEventCodec, event)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AggregateInstance::rehydrate(stream_id, events))
}

fn encode_batch(
    metadata: &ExecutionMetadata,
    events: &[<RentalFleetAggregate as Aggregate>::Event],
) -> Result<EventBatch, CommandExecutionError> {
    let encoded = events
        .iter()
        .enumerate()
        .map(|(ordinal, event)| {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                invalid_request("transfer emitted more events than the supported ordinal range")
            })?;
            <JsonEventCodec as EventCodec<RentalFleetAggregate>>::encode(
                &JsonEventCodec,
                event,
                metadata.event_id(ordinal),
            )
            .map_err(CommandExecutionError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut batch = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        encoded,
    )
    .map_err(|error| invalid_request(error.to_string()))?;
    if let Some(correlation_id) = metadata.correlation_id() {
        batch = batch.with_correlation_id(correlation_id.clone());
    }
    if let Some(causation_id) = metadata.causation_id() {
        batch = batch.with_causation_id(causation_id.clone());
    }
    Ok(batch)
}

fn transaction(
    metadata: &ExecutionMetadata,
    destination_stream: &StreamId,
    source_version: StreamVersion,
    source_batch: EventBatch,
    destination_version: StreamVersion,
    destination_batch: EventBatch,
) -> EventTransaction {
    let mut transaction = EventTransaction::new(
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![
            TransactionParticipant::new(
                metadata.stream_id().clone(),
                expected_version(source_version),
                Some(source_batch),
            ),
            TransactionParticipant::new(
                destination_stream.clone(),
                expected_version(destination_version),
                Some(destination_batch),
            ),
        ],
    );
    if let Some(correlation_id) = metadata.correlation_id() {
        transaction = transaction.with_correlation_id(correlation_id.clone());
    }
    if let Some(causation_id) = metadata.causation_id() {
        transaction = transaction.with_causation_id(causation_id.clone());
    }
    transaction
}

const fn expected_version(version: StreamVersion) -> ExpectedVersion {
    if version.value() == 0 {
        ExpectedVersion::NoStream
    } else {
        ExpectedVersion::Exact(version)
    }
}

fn current_version(history: &[RecordedEvent]) -> StreamVersion {
    history
        .last()
        .map_or(StreamVersion::ZERO, RecordedEvent::stream_version)
}

fn validate_source_stream(stream_id: &StreamId) -> Result<(), EventStoreError> {
    let aggregate_type = RentalFleetAggregate::aggregate_type();
    if stream_id.aggregate_type().as_str() == aggregate_type.as_ref() {
        Ok(())
    } else {
        Err(invalid_request(format!(
            "rental-fleet transfer cannot use source stream type {}",
            stream_id.aggregate_type()
        )))
    }
}

fn invalid_request(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}
