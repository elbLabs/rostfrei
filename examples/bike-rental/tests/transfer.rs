use std::{
    env,
    error::Error,
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use bike_rental::{
    application::{TransferBicycleHandler, TransferBicycleOutcome},
    demo::{apply_demo_fixture, demo_stream},
    rental_fleet::{
        self, BicycleCondition, BicycleId, BicycleStatus, BicycleTransferRejectionReason,
        BicycleTransferredIn, BicycleTransferredOut, FleetId, RentalFleetAggregate,
        TransferBicycle,
    },
};
use rostfrei::{
    AggregateInstance, AppendOutcome, ApplicationName, CausationId, Command, CommandExecutionError,
    CorrelationId, DomainEvent, EventBatch, EventCodec, EventHistory, EventStore, EventStoreError,
    EventStoreErrorKind, EventTransaction, ExecutionMetadata, ExpectedVersion, InMemoryEventStore,
    JsonCommandPayload, OperationId, RecordedEvent, StreamId, TransactionAppendOutcome,
    TransactionReceipt, command_execution_fingerprint,
};
use rostfrei_core::JsonEventCodec;
use rostfrei_nats::{
    NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, ServerVersion, connect,
    provision_event_store,
};
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const DESTINATION_FLEET_ID: &str = "harbor-fleet";
const TRANSFER_OPERATION_ID: &str = "transfer-bike-42-to-harbor";
const TRANSFER_CORRELATION_ID: &str = "transfer-test-correlation";
const TRANSFER_CAUSATION_ID: &str = "transfer-test-causation";

#[tokio::test]
async fn handler_atomically_transfers_and_enforces_operation_identity() -> TestResult {
    let store = InMemoryEventStore::new();
    apply_demo_fixture(&store).await?;

    let original_metadata = verify_success_and_exact_replay(store.clone()).await?;
    let changed_command = transfer_command("bike-99")?;
    let handler = TransferBicycleHandler::new(store.clone());

    let mismatch = handler
        .handle(original_metadata, &changed_command)
        .await
        .err()
        .ok_or_else(|| {
            io::Error::other("changed command with original metadata unexpectedly exact-replayed")
        })?;
    ensure_store_error_kind(
        mismatch,
        EventStoreErrorKind::InvalidRequest,
        "changed command with original metadata was not rejected as invalid",
    )?;

    let identity_conflict = handler
        .handle(transfer_metadata(&changed_command)?, &changed_command)
        .await
        .err()
        .ok_or_else(|| io::Error::other("changed command unexpectedly reused the operation"))?;
    ensure_store_error_kind(
        identity_conflict,
        EventStoreErrorKind::IdentityConflict,
        "changed command with recomputed fingerprint did not produce an identity conflict",
    )?;
    ensure(
        store.load(&demo_stream()).await?.len() == 2,
        "metadata mismatch or identity conflict appended to the source stream",
    )?;
    ensure(
        store.load(&destination_stream()?).await?.len() == 1,
        "metadata mismatch or identity conflict appended to the destination stream",
    )
}

#[tokio::test]
async fn rejected_transfer_appends_neither_participant() -> TestResult {
    let store = InMemoryEventStore::new();
    apply_demo_fixture(&store).await?;
    let destination_stream = destination_stream()?;
    let source_before = store.load(&demo_stream()).await?;
    let destination_before = store.load(&destination_stream).await?;
    let command = transfer_command("missing-bike")?;

    let outcome = TransferBicycleHandler::new(store.clone())
        .handle(
            metadata_for_operation("reject-missing-bike-transfer", &command)?,
            &command,
        )
        .await?;
    let TransferBicycleOutcome::Rejected(rejection) = outcome else {
        return Err(io::Error::other("missing bicycle transfer was not rejected").into());
    };
    ensure(
        rejection.reason == BicycleTransferRejectionReason::SourceBicycleMissing,
        "transfer was rejected for the wrong reason",
    )?;
    ensure(
        store.load(&demo_stream()).await? == source_before,
        "rejected transfer appended to the source stream",
    )?;
    ensure(
        store.load(&destination_stream).await? == destination_before,
        "rejected transfer appended to the destination stream",
    )
}

#[tokio::test]
async fn handler_retries_one_transaction_conflict_without_duplicate_events() -> TestResult {
    let inner = InMemoryEventStore::new();
    apply_demo_fixture(&inner).await?;
    let store = FaultInjectingEventStore::new(inner.clone(), TransactionFault::ConflictOnce);

    verify_success_and_exact_replay(store.clone()).await?;

    ensure(
        store.transaction_attempts() == 2,
        "handler did not retry exactly once after the injected conflict",
    )?;
    verify_persisted_event_counts(&inner).await
}

#[tokio::test]
async fn failed_transaction_append_leaves_both_streams_unchanged() -> TestResult {
    let inner = InMemoryEventStore::new();
    apply_demo_fixture(&inner).await?;
    let destination_stream = destination_stream()?;
    let source_before = inner.load(&demo_stream()).await?;
    let destination_before = inner.load(&destination_stream).await?;
    let store = FaultInjectingEventStore::new(inner.clone(), TransactionFault::FailAlways);
    let command = transfer_command("bike-42")?;

    let error = TransferBicycleHandler::new(store.clone())
        .handle(transfer_metadata(&command)?, &command)
        .await
        .err()
        .ok_or_else(|| io::Error::other("injected transaction failure unexpectedly succeeded"))?;

    ensure_store_error_kind(
        error,
        EventStoreErrorKind::Unavailable,
        "handler did not propagate the injected transaction failure",
    )?;
    ensure(
        store.transaction_attempts() == 1,
        "handler made an unexpected number of failing transaction attempts",
    )?;
    ensure(
        inner.load(&demo_stream()).await? == source_before,
        "failed transaction partially appended to the source stream",
    )?;
    ensure(
        inner.load(&destination_stream).await? == destination_before,
        "failed transaction partially appended to the destination stream",
    )
}

#[tokio::test]
#[ignore = "requires NATS 2.12.1+"]
async fn handler_atomically_transfers_with_real_nats() -> TestResult {
    let nats_url = required_nats_url()?;
    let unique = Uuid::now_v7();
    let application = ApplicationName::new(format!("transfer-test-{unique}"))?;
    let context = application.test_bounded_context(format!("bike-rental-{unique}"))?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("transfer-test-{unique}"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let result: TestResult = async {
        provision_event_store(connection.jetstream(), &config).await?;
        let store = NatsEventStore::connect(connection.jetstream().clone(), config.clone()).await?;
        apply_demo_fixture(&store).await?;
        verify_success_and_exact_replay(store).await?;
        Ok(())
    }
    .await;
    let cleanup = connection
        .delete_stream_if_exists(config.stream_name())
        .await;
    let drain = connection.drain().await;

    result?;
    cleanup?;
    drain?;
    Ok(())
}

async fn verify_success_and_exact_replay<S>(store: S) -> TestResult<ExecutionMetadata>
where
    S: EventStore + Clone,
{
    let destination_stream = destination_stream()?;
    ensure(
        store.load(&destination_stream).await?.is_empty(),
        "destination stream must be absent before transfer",
    )?;
    let command = transfer_command("bike-42")?;
    let metadata = transfer_metadata(&command)?;
    let handler = TransferBicycleHandler::new(store.clone());

    let outcome = handler.handle(metadata.clone(), &command).await?;
    let TransferBicycleOutcome::Accepted(receipt) = outcome else {
        return Err(io::Error::other("first transfer was not accepted and appended").into());
    };
    verify_receipt(&receipt, &destination_stream, &metadata)?;

    let source_history = store.load(&demo_stream()).await?;
    let destination_history = store.load(&destination_stream).await?;
    ensure(
        source_history.len() == 2,
        "source stream does not contain fixture and outgoing events",
    )?;
    ensure(
        destination_history.len() == 1,
        "destination stream does not contain exactly one incoming event",
    )?;
    verify_recorded_control_ids(&source_history, &destination_history, &metadata)?;
    verify_replayed_state(&source_history, &destination_history, &destination_stream)?;

    let replay = handler.handle(metadata.clone(), &command).await?;
    let TransferBicycleOutcome::ExactReplay(replay_receipt) = replay else {
        return Err(
            io::Error::other("identical transfer was not reported as an exact replay").into(),
        );
    };
    ensure(
        replay_receipt == receipt,
        "exact replay returned a different transaction receipt",
    )?;
    ensure(
        store.load(&demo_stream()).await? == source_history,
        "exact replay appended to the source stream",
    )?;
    ensure(
        store.load(&destination_stream).await? == destination_history,
        "exact replay appended to the destination stream",
    )?;
    Ok(metadata)
}

fn verify_receipt(
    receipt: &TransactionReceipt,
    destination_stream: &StreamId,
    metadata: &ExecutionMetadata,
) -> TestResult {
    let streams = receipt.streams();
    let source = streams
        .first()
        .ok_or_else(|| io::Error::other("transaction receipt has no source participant"))?;
    let destination = streams
        .get(1)
        .ok_or_else(|| io::Error::other("transaction receipt has no destination participant"))?;
    ensure(
        streams.len() == 2,
        "transaction receipt does not have exactly two participants",
    )?;
    ensure(
        source.stream_id() == &demo_stream(),
        "source is not the first receipt participant",
    )?;
    ensure(
        destination.stream_id() == destination_stream,
        "destination is not the second receipt participant",
    )?;
    ensure(
        receipt.correlation_id() == metadata.correlation_id()
            && receipt.causation_id() == metadata.causation_id(),
        "transaction receipt did not preserve correlation and causation IDs",
    )?;
    ensure(
        source.events().len() == 1
            && source
                .events()
                .first()
                .is_some_and(|event| event.event_type() == BicycleTransferredOut::LOCAL_ID),
        "source receipt participant does not contain the outgoing event",
    )?;
    ensure(
        destination.events().len() == 1
            && destination
                .events()
                .first()
                .is_some_and(|event| event.event_type() == BicycleTransferredIn::LOCAL_ID),
        "destination receipt participant does not contain the incoming event",
    )
}

fn verify_recorded_control_ids(
    source_history: &[RecordedEvent],
    destination_history: &[RecordedEvent],
    metadata: &ExecutionMetadata,
) -> TestResult {
    let outgoing = source_history
        .last()
        .ok_or_else(|| io::Error::other("source history has no outgoing event"))?;
    let incoming = destination_history
        .first()
        .ok_or_else(|| io::Error::other("destination history has no incoming event"))?;
    for event in [outgoing, incoming] {
        ensure(
            event.correlation_id() == metadata.correlation_id()
                && event.causation_id() == metadata.causation_id(),
            "recorded transaction event did not preserve correlation and causation IDs",
        )?;
    }
    Ok(())
}

fn verify_replayed_state(
    source_history: &[RecordedEvent],
    destination_history: &[RecordedEvent],
    destination_stream: &StreamId,
) -> TestResult {
    let source = rehydrate(demo_stream(), source_history)?;
    let destination = rehydrate(destination_stream.clone(), destination_history)?;
    let transferred_id = bicycle_id("bike-42")?;
    let retained_id = bicycle_id("bike-99")?;

    ensure(
        source.state().bicycles().len() == 1
            && source
                .state()
                .bicycles()
                .first()
                .is_some_and(|bicycle| bicycle.bicycle_id() == &retained_id),
        "source replay did not remove only the transferred bicycle",
    )?;
    let transferred = destination
        .state()
        .bicycles()
        .first()
        .ok_or_else(|| io::Error::other("destination replay has no transferred bicycle"))?;
    ensure(
        destination.state().bicycles().len() == 1
            && transferred.bicycle_id() == &transferred_id
            && transferred.status() == BicycleStatus::Available
            && transferred.condition() == BicycleCondition::Serviceable,
        "destination replay did not restore the transferred bicycle state",
    )
}

async fn verify_persisted_event_counts(store: &InMemoryEventStore) -> TestResult {
    let source_history = store.load(&demo_stream()).await?;
    let destination_history = store.load(&destination_stream()?).await?;
    let outgoing_count = source_history
        .iter()
        .filter(|event| event.event_type() == BicycleTransferredOut::LOCAL_ID)
        .count();
    let incoming_count = destination_history
        .iter()
        .filter(|event| event.event_type() == BicycleTransferredIn::LOCAL_ID)
        .count();
    ensure(
        outgoing_count == 1 && incoming_count == 1,
        "conflict retry did not commit exactly one outgoing and one incoming event",
    )
}

fn rehydrate(
    stream_id: StreamId,
    history: &[RecordedEvent],
) -> TestResult<AggregateInstance<RentalFleetAggregate>> {
    let events = history
        .iter()
        .map(|event| {
            <JsonEventCodec as EventCodec<RentalFleetAggregate>>::decode(&JsonEventCodec, event)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AggregateInstance::rehydrate(stream_id, events))
}

fn transfer_metadata(command: &TransferBicycle) -> TestResult<ExecutionMetadata> {
    metadata_for_operation(TRANSFER_OPERATION_ID, command)
}

fn metadata_for_operation(
    operation_id: &str,
    command: &TransferBicycle,
) -> TestResult<ExecutionMetadata> {
    let source = demo_stream();
    let payload = command
        .encode_json()
        .map_err(|error| io::Error::other(format!("failed to encode transfer command: {error}")))?;
    let fingerprint = command_execution_fingerprint(
        source.aggregate_type().as_str(),
        source.aggregate_id().as_str(),
        TransferBicycle::LOCAL_ID,
        TransferBicycle::SCHEMA_VERSION,
        &payload,
    )?;
    Ok(
        ExecutionMetadata::new(source, OperationId::new(operation_id)?, fingerprint)
            .with_correlation_id(CorrelationId::new(TRANSFER_CORRELATION_ID)?)
            .with_causation_id(CausationId::new(TRANSFER_CAUSATION_ID)?),
    )
}

fn transfer_command(bicycle: &str) -> TestResult<TransferBicycle> {
    Ok(TransferBicycle {
        bicycle_id: bicycle_id(bicycle)?,
        to_fleet_id: fleet_id(DESTINATION_FLEET_ID)?,
    })
}

fn destination_stream() -> TestResult<StreamId> {
    rental_fleet::stream_id(DESTINATION_FLEET_ID).map_err(Into::into)
}

fn bicycle_id(value: &str) -> TestResult<BicycleId> {
    BicycleId::new(value)
        .ok_or_else(|| io::Error::other(format!("invalid bicycle identity: {value}")))
        .map_err(Into::into)
}

fn fleet_id(value: &str) -> TestResult<FleetId> {
    FleetId::new(value)
        .ok_or_else(|| io::Error::other(format!("invalid fleet identity: {value}")))
        .map_err(Into::into)
}

fn ensure_store_error_kind(
    error: CommandExecutionError,
    expected: EventStoreErrorKind,
    message: &'static str,
) -> TestResult {
    let CommandExecutionError::Store(store_error) = error else {
        return Err(io::Error::other(message).into());
    };
    ensure(store_error.kind() == expected, message)
}

#[derive(Clone, Copy)]
enum TransactionFault {
    ConflictOnce,
    FailAlways,
}

#[derive(Clone)]
struct FaultInjectingEventStore {
    inner: InMemoryEventStore,
    fault: TransactionFault,
    transaction_attempts: Arc<AtomicUsize>,
}

impl FaultInjectingEventStore {
    fn new(inner: InMemoryEventStore, fault: TransactionFault) -> Self {
        Self {
            inner,
            fault,
            transaction_attempts: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn transaction_attempts(&self) -> usize {
        self.transaction_attempts.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl EventHistory for FaultInjectingEventStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(stream_id).await
    }
}

#[async_trait]
impl EventStore for FaultInjectingEventStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.inner.append(stream_id, expected_version, batch).await
    }

    async fn load_transaction_receipt(
        &self,
        primary_stream_id: &StreamId,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner
            .load_transaction_receipt(primary_stream_id, operation_id)
            .await
    }

    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        let attempt = self.transaction_attempts.fetch_add(1, Ordering::SeqCst);
        match self.fault {
            TransactionFault::ConflictOnce if attempt == 0 => Err(EventStoreError::new(
                EventStoreErrorKind::Conflict,
                "injected transaction conflict",
            )),
            TransactionFault::FailAlways => Err(EventStoreError::new(
                EventStoreErrorKind::Unavailable,
                "injected transaction failure",
            )),
            TransactionFault::ConflictOnce => self.inner.append_transaction(transaction).await,
        }
    }
}

fn required_nats_url() -> TestResult<String> {
    match env::var("ROSTFREI_NATS_URL") {
        Ok(url) if !url.trim().is_empty() => Ok(url),
        Ok(_) => Err(io::Error::other("ROSTFREI_NATS_URL must not be empty").into()),
        Err(error) => Err(io::Error::other(format!(
            "ROSTFREI_NATS_URL is required for this ignored real-NATS test: {error}"
        ))
        .into()),
    }
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
