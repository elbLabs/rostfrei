use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use thiserror::Error;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::{
    Aggregate, AggregateId, AggregateInstance, AggregateType, AppendOutcome, Event, EventBatch,
    EventCodec, EventCodecError, EventHistory, EventStore, EventStoreError, EventStoreErrorKind,
    EventTransaction, ExecutionMetadata, ExpectedVersion, JsonEventCodec, NewEvent, RecordedEvent,
    StreamId, StreamVersion, TransactionAppendOutcome, TransactionParticipant, TransactionReceipt,
};

const DEFAULT_MAX_CONFLICT_RETRIES: usize = 3;
static NEXT_COMMAND_ATTEMPT: AtomicU64 = AtomicU64::new(1);

pub struct Executor<S, C = JsonEventCodec> {
    store: S,
    codec: C,
    maximum_conflict_retries: usize,
}

impl<S> Executor<S, JsonEventCodec> {
    pub const fn new(store: S) -> Self {
        Self::with_codec(store, JsonEventCodec)
    }
}

impl<S, C> Executor<S, C> {
    pub const fn with_codec(store: S, codec: C) -> Self {
        Self {
            store,
            codec,
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

    pub const fn codec(&self) -> &C {
        &self.codec
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandDecision<Rejection> {
    Accepted,
    Rejected(Rejection),
}

pub type CommandHandlingResult<Rejection> =
    Result<CommandDecision<Rejection>, CommandExecutionError>;

/// Handles a command against its explicit primary aggregate and any additional aggregates loaded
/// through the command context.
#[async_trait]
pub trait CommandHandler<Command>: Aggregate + Send + Sync
where
    Command: Sync,
    Self::State: Send,
    Self::Event: Send,
{
    type Rejection: Send;

    async fn handle(
        command: &Command,
        primary: &mut AggregateInstance<Self>,
        context: &mut CommandContext<'_>,
    ) -> CommandHandlingResult<Self::Rejection>;
}

/// An additional aggregate loaded during one command-handling attempt.
pub struct LoadedAggregate<A: Aggregate> {
    aggregate: AggregateInstance<A>,
    base_version: StreamVersion,
    attempt: u64,
}

impl<A: Aggregate> LoadedAggregate<A> {
    pub const fn aggregate(&self) -> &AggregateInstance<A> {
        &self.aggregate
    }

    pub const fn aggregate_mut(&mut self) -> &mut AggregateInstance<A> {
        &mut self.aggregate
    }

    pub const fn base_version(&self) -> StreamVersion {
        self.base_version
    }
}

struct IncludedParticipant {
    stream_id: StreamId,
    base_version: StreamVersion,
    events: Vec<NewEvent>,
}

/// Per-attempt access to additional aggregate streams.
///
/// Additional aggregates deliberately use [`JsonEventCodec`], and therefore their event type must
/// implement [`Event`]. The executor's configured codec continues to be used for the primary.
pub struct CommandContext<'a> {
    history: &'a dyn EventHistory,
    metadata: &'a ExecutionMetadata,
    attempt: u64,
    loaded_streams: HashSet<StreamId>,
    load_order: Vec<(StreamId, StreamVersion)>,
    pending_streams: HashSet<StreamId>,
    participants: Vec<IncludedParticipant>,
}

impl<'a> CommandContext<'a> {
    pub fn new(history: &'a dyn EventHistory, metadata: &'a ExecutionMetadata) -> Self {
        Self {
            history,
            metadata,
            attempt: NEXT_COMMAND_ATTEMPT.fetch_add(1, Ordering::Relaxed),
            loaded_streams: HashSet::new(),
            load_order: Vec::new(),
            pending_streams: HashSet::new(),
            participants: Vec::new(),
        }
    }

    pub const fn metadata(&self) -> &ExecutionMetadata {
        self.metadata
    }

    /// Loads and replays an additional aggregate with the standard JSON event codec.
    pub async fn load<A>(
        &mut self,
        aggregate_id: &str,
    ) -> Result<LoadedAggregate<A>, CommandExecutionError>
    where
        A: Aggregate,
        A::Event: Event,
    {
        let stream_id = stream_id_for::<A>(aggregate_id)?;
        if &stream_id == self.metadata.stream_id() {
            return Err(invalid_request(
                "the primary aggregate is already provided explicitly and cannot be loaded",
            )
            .into());
        }
        if self.loaded_streams.contains(&stream_id) {
            return Err(invalid_request(
                "an additional aggregate stream may be loaded only once per command attempt",
            )
            .into());
        }

        let history = self.history.load(&stream_id).await?;
        validate_history(&stream_id, &history)?;
        let codec = JsonEventCodec;
        let mut events = Vec::with_capacity(history.len());
        for event in &history {
            events.push(<JsonEventCodec as EventCodec<A>>::decode(&codec, event)?);
        }
        let base_version = current_version(&history);
        self.loaded_streams.insert(stream_id.clone());
        self.load_order.push((stream_id.clone(), base_version));
        self.pending_streams.insert(stream_id.clone());
        Ok(LoadedAggregate {
            aggregate: AggregateInstance::rehydrate(stream_id, events),
            base_version,
            attempt: self.attempt,
        })
    }

    /// Includes a loaded aggregate in the atomic command result.
    ///
    /// An included aggregate with no uncommitted events is retained as a transaction read guard.
    /// Additional events are encoded with [`JsonEventCodec`].
    pub fn include<A>(&mut self, loaded: LoadedAggregate<A>) -> Result<(), CommandExecutionError>
    where
        A: Aggregate,
        A::Event: Event,
    {
        let LoadedAggregate {
            aggregate,
            base_version,
            attempt,
        } = loaded;
        let stream_id = aggregate.stream_id().clone();
        if &stream_id == self.metadata.stream_id() {
            return Err(invalid_request(
                "the primary aggregate cannot be included as an additional participant",
            )
            .into());
        }
        if attempt != self.attempt {
            return Err(invalid_request(
                "a loaded aggregate belongs to a different command-handling attempt",
            )
            .into());
        }
        if self
            .participants
            .iter()
            .any(|participant| participant.stream_id == stream_id)
        {
            return Err(invalid_request(
                "an additional aggregate stream cannot be included more than once",
            )
            .into());
        }
        if !self.pending_streams.remove(&stream_id) {
            return Err(invalid_request(
                "the aggregate was not loaded by this command context or was already included",
            )
            .into());
        }

        let codec = JsonEventCodec;
        let commit_id = derive_commit_id(&stream_id, self.metadata.operation_id());
        let mut events = Vec::with_capacity(aggregate.uncommitted_events().len());
        for (ordinal, event) in aggregate.uncommitted_events().iter().enumerate() {
            let ordinal = supported_ordinal(ordinal)?;
            events.push(<JsonEventCodec as EventCodec<A>>::encode(
                &codec,
                event,
                derive_event_id(&commit_id, ordinal),
            )?);
        }
        self.participants.push(IncludedParticipant {
            stream_id,
            base_version,
            events,
        });
        Ok(())
    }

    fn finish(self) -> Result<Vec<IncludedParticipant>, CommandExecutionError> {
        if !self.pending_streams.is_empty() {
            return Err(invalid_request(
                "accepted command left one or more loaded aggregate streams unincluded",
            )
            .into());
        }
        Ok(self.participants)
    }

    fn rejected_participants(self) -> Vec<IncludedParticipant> {
        let mut included: HashMap<_, _> = self
            .participants
            .into_iter()
            .map(|participant| (participant.stream_id.clone(), participant))
            .collect();
        self.load_order
            .into_iter()
            .map(|(stream_id, base_version)| {
                included.remove(&stream_id).unwrap_or(IncludedParticipant {
                    stream_id,
                    base_version,
                    events: Vec::new(),
                })
            })
            .collect()
    }
}

/// The completed business outcome of command execution or an infrastructure failure.
pub type CommandResult<Rejection> = Result<CommandOutcome<Rejection>, CommandExecutionError>;

#[derive(Clone, Debug, Eq, PartialEq)]
/// A command's accepted or rejected business outcome.
pub enum CommandOutcome<Rejection> {
    Accepted(CommandReceipt),
    Rejected(Rejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Evidence returned for an accepted command.
///
/// `NoEvents` is not persisted and therefore is not a durable idempotency receipt.
pub enum CommandReceipt {
    Appended(Vec<RecordedEvent>),
    ExactReplay(Vec<RecordedEvent>),
    NoEvents,
}

impl CommandReceipt {
    pub fn events(&self) -> &[RecordedEvent] {
        match self {
            Self::Appended(events) | Self::ExactReplay(events) => events,
            Self::NoEvents => &[],
        }
    }

    pub const fn is_exact_replay(&self) -> bool {
        matches!(self, Self::ExactReplay(_))
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
/// A codec or event-store failure that prevented command execution from completing.
pub enum CommandExecutionError {
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(transparent)]
    Codec(#[from] EventCodecError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulatedParticipant {
    stream_id: StreamId,
    base_version: StreamVersion,
    events: Vec<NewEvent>,
    read_guard: bool,
}

impl SimulatedParticipant {
    pub const fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub const fn base_version(&self) -> StreamVersion {
        self.base_version
    }

    pub fn events(&self) -> &[NewEvent] {
        &self.events
    }

    pub const fn is_read_guard(&self) -> bool {
        self.read_guard
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulationOutcome<Rejection> {
    base_version: StreamVersion,
    decision: SimulationDecision<Rejection>,
    participants: Vec<SimulatedParticipant>,
}

impl<Rejection> SimulationOutcome<Rejection> {
    pub const fn base_version(&self) -> StreamVersion {
        self.base_version
    }

    pub const fn decision(&self) -> &SimulationDecision<Rejection> {
        &self.decision
    }

    /// Returns the primary stream first, followed by explicitly included streams and read guards.
    pub fn participants(&self) -> &[SimulatedParticipant] {
        &self.participants
    }

    pub fn into_parts(self) -> (StreamVersion, SimulationDecision<Rejection>) {
        (self.base_version, self.decision)
    }

    pub fn into_full_parts(
        self,
    ) -> (
        StreamVersion,
        SimulationDecision<Rejection>,
        Vec<SimulatedParticipant>,
    ) {
        (self.base_version, self.decision, self.participants)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimulationDecision<Rejection> {
    Accepted(Vec<NewEvent>),
    Rejected(Rejection),
}

impl<Rejection> SimulationDecision<Rejection> {
    pub const fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted(_))
    }

    pub fn events(&self) -> Option<&[NewEvent]> {
        match self {
            Self::Accepted(events) => Some(events),
            Self::Rejected(_) => None,
        }
    }

    pub const fn rejection(&self) -> Option<&Rejection> {
        match self {
            Self::Accepted(_) => None,
            Self::Rejected(rejection) => Some(rejection),
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SimulationError {
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(transparent)]
    Codec(#[from] EventCodecError),
}

impl From<SimulationError> for CommandExecutionError {
    fn from(error: SimulationError) -> Self {
        match error {
            SimulationError::Store(error) => Self::Store(error),
            SimulationError::Codec(error) => Self::Codec(error),
        }
    }
}

impl From<CommandExecutionError> for SimulationError {
    fn from(error: CommandExecutionError) -> Self {
        match error {
            CommandExecutionError::Store(error) => Self::Store(error),
            CommandExecutionError::Codec(error) => Self::Codec(error),
        }
    }
}

impl<S, C> Executor<S, C>
where
    S: EventHistory,
{
    pub async fn rehydrate<A>(
        &self,
        stream_id: &StreamId,
    ) -> Result<AggregateInstance<A>, SimulationError>
    where
        A: Aggregate,
        C: EventCodec<A>,
    {
        self.load_and_replay::<A>(stream_id)
            .await
            .map(|(aggregate, _)| aggregate)
    }

    pub async fn simulate<A, Command>(
        &self,
        metadata: ExecutionMetadata,
        command: &Command,
    ) -> Result<SimulationOutcome<<A as CommandHandler<Command>>::Rejection>, SimulationError>
    where
        A: CommandHandler<Command>,
        A::State: Send,
        A::Event: Send,
        C: EventCodec<A>,
        Command: Sync,
    {
        let (mut primary, history) = self.load_and_replay::<A>(metadata.stream_id()).await?;
        let base_version = current_version(&history);
        let mut context = CommandContext::new(&self.store, &metadata);
        let handling = A::handle(command, &mut primary, &mut context).await?;

        let (decision, extras) = match handling {
            CommandDecision::Accepted => {
                let extras = context.finish()?;
                let primary_events = self.encode_primary(&metadata, &primary)?;
                if !extras.is_empty() && primary_events.is_empty() {
                    return Err(invalid_request(
                        "a multi-stream command must emit at least one event on its primary aggregate",
                    )
                    .into());
                }
                (SimulationDecision::Accepted(primary_events), extras)
            }
            CommandDecision::Rejected(rejection) => (
                SimulationDecision::Rejected(rejection),
                context.rejected_participants(),
            ),
        };

        let primary_events = decision.events().map_or_else(Vec::new, ToOwned::to_owned);
        let mut participants = Vec::with_capacity(extras.len().saturating_add(1));
        participants.push(SimulatedParticipant {
            stream_id: metadata.stream_id().clone(),
            base_version,
            events: primary_events,
            read_guard: false,
        });
        participants.extend(extras.into_iter().map(|participant| SimulatedParticipant {
            stream_id: participant.stream_id,
            base_version: participant.base_version,
            read_guard: participant.events.is_empty(),
            events: participant.events,
        }));

        Ok(SimulationOutcome {
            base_version,
            decision,
            participants,
        })
    }

    async fn load_and_replay<A>(
        &self,
        stream_id: &StreamId,
    ) -> Result<(AggregateInstance<A>, Vec<RecordedEvent>), SimulationError>
    where
        A: Aggregate,
        C: EventCodec<A>,
    {
        validate_aggregate_type::<A>(stream_id)?;
        let history = self.store.load(stream_id).await?;
        validate_history(stream_id, &history)?;

        let mut events = Vec::with_capacity(history.len());
        for event in &history {
            events.push(self.codec.decode(event)?);
        }
        Ok((
            AggregateInstance::rehydrate(stream_id.clone(), events),
            history,
        ))
    }

    fn encode_primary<A>(
        &self,
        metadata: &ExecutionMetadata,
        aggregate: &AggregateInstance<A>,
    ) -> Result<Vec<NewEvent>, CommandExecutionError>
    where
        A: Aggregate,
        C: EventCodec<A>,
    {
        let mut encoded = Vec::with_capacity(aggregate.uncommitted_events().len());
        for (ordinal, event) in aggregate.uncommitted_events().iter().enumerate() {
            encoded.push(
                self.codec
                    .encode(event, metadata.event_id(supported_ordinal(ordinal)?))?,
            );
        }
        Ok(encoded)
    }
}

impl<S, C> Executor<S, C>
where
    S: EventStore,
{
    pub async fn execute<A, Command>(
        &self,
        metadata: ExecutionMetadata,
        command: &Command,
    ) -> CommandResult<<A as CommandHandler<Command>>::Rejection>
    where
        A: CommandHandler<Command>,
        A::State: Send,
        A::Event: Send,
        C: EventCodec<A>,
        Command: Sync,
    {
        let mut remaining_conflict_retries = self.maximum_conflict_retries;
        loop {
            if let Some(receipt) = reconcile_existing(&self.store, &metadata).await? {
                return Ok(CommandOutcome::Accepted(receipt));
            }

            let (mut primary, history) = self.load_and_replay::<A>(metadata.stream_id()).await?;
            if let Some(receipt) = receipt_from_history(&metadata, &history)? {
                return Ok(CommandOutcome::Accepted(receipt));
            }

            let mut context = CommandContext::new(&self.store, &metadata);
            let handling = A::handle(command, &mut primary, &mut context).await?;
            let extras = match handling {
                CommandDecision::Accepted => context.finish()?,
                CommandDecision::Rejected(rejection) => {
                    if let Some(receipt) = reconcile_existing(&self.store, &metadata).await? {
                        return Ok(CommandOutcome::Accepted(receipt));
                    }
                    return Ok(CommandOutcome::Rejected(rejection));
                }
            };
            let primary_events = self.encode_primary(&metadata, &primary)?;
            let base_version = current_version(&history);
            let persistence = if extras.is_empty() {
                append_primary(&self.store, &metadata, base_version, primary_events).await?
            } else {
                append_transaction(&self.store, &metadata, base_version, primary_events, extras)
                    .await?
            };

            match persistence {
                PersistenceAttempt::Completed(receipt) => {
                    return Ok(CommandOutcome::Accepted(receipt));
                }
                PersistenceAttempt::Conflict(_) if remaining_conflict_retries > 0 => {
                    remaining_conflict_retries = remaining_conflict_retries.saturating_sub(1);
                }
                PersistenceAttempt::Conflict(error) => return Err(error.into()),
            }
        }
    }
}

enum PersistenceAttempt {
    Completed(CommandReceipt),
    Conflict(EventStoreError),
}

async fn append_primary<S: EventStore>(
    store: &S,
    metadata: &ExecutionMetadata,
    base_version: StreamVersion,
    events: Vec<NewEvent>,
) -> Result<PersistenceAttempt, CommandExecutionError> {
    let Some(batch) = prepare_batch_for_stream(metadata, metadata.stream_id(), events)? else {
        return Ok(PersistenceAttempt::Completed(CommandReceipt::NoEvents));
    };
    match store
        .append(metadata.stream_id(), expected_version(base_version), batch)
        .await
    {
        Ok(AppendOutcome::Appended(events)) => Ok(PersistenceAttempt::Completed(
            CommandReceipt::Appended(events),
        )),
        Ok(AppendOutcome::ExactReplay(events)) => Ok(PersistenceAttempt::Completed(
            CommandReceipt::ExactReplay(events),
        )),
        Err(error) if error.kind() == EventStoreErrorKind::Conflict => {
            Ok(PersistenceAttempt::Conflict(error))
        }
        Err(error) => Err(error.into()),
    }
}

async fn append_transaction<S: EventStore>(
    store: &S,
    metadata: &ExecutionMetadata,
    primary_base_version: StreamVersion,
    primary_events: Vec<NewEvent>,
    extras: Vec<IncludedParticipant>,
) -> Result<PersistenceAttempt, CommandExecutionError> {
    let Some(primary_batch) =
        prepare_batch_for_stream(metadata, metadata.stream_id(), primary_events)?
    else {
        return Err(invalid_request(
            "a multi-stream command must emit at least one event on its primary aggregate",
        )
        .into());
    };
    let mut participants = Vec::with_capacity(extras.len().saturating_add(1));
    participants.push(TransactionParticipant::new(
        metadata.stream_id().clone(),
        expected_version(primary_base_version),
        Some(primary_batch),
    ));
    for participant in extras {
        let batch = prepare_batch_for_stream(metadata, &participant.stream_id, participant.events)?;
        participants.push(TransactionParticipant::new(
            participant.stream_id,
            expected_version(participant.base_version),
            batch,
        ));
    }
    let mut transaction = EventTransaction::new(
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        participants,
    );
    if let Some(correlation_id) = metadata.correlation_id() {
        transaction = transaction.with_correlation_id(correlation_id.clone());
    }
    if let Some(causation_id) = metadata.causation_id() {
        transaction = transaction.with_causation_id(causation_id.clone());
    }

    match store.append_transaction(transaction).await {
        Ok(TransactionAppendOutcome::Appended(receipt)) => Ok(PersistenceAttempt::Completed(
            CommandReceipt::Appended(receipt.events()),
        )),
        Ok(TransactionAppendOutcome::ExactReplay(receipt)) => Ok(PersistenceAttempt::Completed(
            CommandReceipt::ExactReplay(receipt.events()),
        )),
        Err(error) if error.kind() == EventStoreErrorKind::Conflict => {
            Ok(PersistenceAttempt::Conflict(error))
        }
        Err(error) => Err(error.into()),
    }
}

async fn reconcile_existing<S: EventStore>(
    store: &S,
    metadata: &ExecutionMetadata,
) -> Result<Option<CommandReceipt>, CommandExecutionError> {
    if let Some(receipt) = store
        .load_transaction_receipt(metadata.stream_id(), metadata.operation_id())
        .await?
    {
        return transaction_replay(metadata, &receipt).map(Some);
    }
    let history = store.load(metadata.stream_id()).await?;
    validate_history(metadata.stream_id(), &history)?;
    receipt_from_history(metadata, &history)
}

fn transaction_replay(
    metadata: &ExecutionMetadata,
    receipt: &TransactionReceipt,
) -> Result<CommandReceipt, CommandExecutionError> {
    let primary_matches = receipt.primary_stream_id() == Some(metadata.stream_id())
        && receipt.streams().first().is_some_and(|stream| {
            !stream.events().is_empty()
                && stream
                    .events()
                    .iter()
                    .all(|event| event.commit_id() == metadata.commit_id())
        });
    if receipt.operation_id() == metadata.operation_id()
        && receipt.operation_fingerprint() == metadata.operation_fingerprint()
        && receipt.correlation_id() == metadata.correlation_id()
        && receipt.causation_id() == metadata.causation_id()
        && primary_matches
    {
        return Ok(CommandReceipt::ExactReplay(receipt.events()));
    }
    Err(EventStoreError::new(
        EventStoreErrorKind::IdentityConflict,
        "operation identity was reused with different transaction metadata",
    )
    .into())
}

fn receipt_from_history(
    metadata: &ExecutionMetadata,
    history: &[RecordedEvent],
) -> Result<Option<CommandReceipt>, CommandExecutionError> {
    let prior_operation: Vec<_> = history
        .iter()
        .filter(|event| event.operation_id() == metadata.operation_id())
        .cloned()
        .collect();
    if prior_operation.is_empty() {
        return Ok(None);
    }
    let exact = prior_operation.iter().all(|event| {
        event.commit_id() == metadata.commit_id()
            && event.operation_fingerprint() == metadata.operation_fingerprint()
            && event.correlation_id() == metadata.correlation_id()
            && event.causation_id() == metadata.causation_id()
    });
    if exact {
        return Ok(Some(CommandReceipt::ExactReplay(prior_operation)));
    }
    Err(EventStoreError::new(
        EventStoreErrorKind::IdentityConflict,
        "operation identity was reused with a different fingerprint or commit",
    )
    .into())
}

fn stream_id_for<A: Aggregate>(aggregate_id: &str) -> Result<StreamId, EventStoreError> {
    let aggregate_type = AggregateType::new(A::aggregate_type().into_owned())
        .map_err(|error| invalid_request(error.to_string()))?;
    let aggregate_id =
        AggregateId::new(aggregate_id).map_err(|error| invalid_request(error.to_string()))?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
}

fn validate_aggregate_type<A: Aggregate>(stream_id: &StreamId) -> Result<(), EventStoreError> {
    let aggregate_type = A::aggregate_type();
    if stream_id.aggregate_type().as_str() != aggregate_type.as_ref() {
        return Err(invalid_request(format!(
            "aggregate type {} cannot execute stream type {}",
            aggregate_type,
            stream_id.aggregate_type()
        )));
    }
    Ok(())
}

fn prepare_batch_for_stream(
    metadata: &ExecutionMetadata,
    stream_id: &StreamId,
    events: Vec<NewEvent>,
) -> Result<Option<EventBatch>, EventStoreError> {
    if events.is_empty() {
        return Ok(None);
    }
    let mut batch = EventBatch::new(
        derive_commit_id(stream_id, metadata.operation_id()),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )
    .map_err(|error| invalid_request(error.to_string()))?;
    if let Some(correlation_id) = metadata.correlation_id() {
        batch = batch.with_correlation_id(correlation_id.clone());
    }
    if let Some(causation_id) = metadata.causation_id() {
        batch = batch.with_causation_id(causation_id.clone());
    }
    Ok(Some(batch))
}

fn expected_version(version: StreamVersion) -> ExpectedVersion {
    if version == StreamVersion::ZERO {
        ExpectedVersion::NoStream
    } else {
        ExpectedVersion::Exact(version)
    }
}

fn supported_ordinal(ordinal: usize) -> Result<u32, EventStoreError> {
    u32::try_from(ordinal).map_err(|_| {
        invalid_request("command emitted more events than the supported ordinal range")
    })
}

fn current_version(history: &[RecordedEvent]) -> StreamVersion {
    history
        .last()
        .map_or(StreamVersion::ZERO, RecordedEvent::stream_version)
}

fn validate_history(
    stream_id: &StreamId,
    history: &[RecordedEvent],
) -> Result<(), EventStoreError> {
    let mut event_ids = HashSet::with_capacity(history.len());
    let mut seen_commits = HashSet::new();
    let mut operations = HashMap::new();
    let mut current_commit: Option<(&crate::CommitId, u32, u32)> = None;

    for (index, event) in history.iter().enumerate() {
        if event.stream_id() != stream_id {
            return Err(corrupt(
                "loaded history contains an event from another stream",
            ));
        }
        let expected_position = u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .map(StreamVersion::new)
            .ok_or_else(|| corrupt("loaded history exceeds the supported version range"))?;
        if event.stream_version() != expected_position {
            return Err(corrupt("loaded history has non-contiguous stream versions"));
        }
        if !event_ids.insert(event.event_id()) {
            return Err(corrupt(
                "loaded history contains a duplicate event identity",
            ));
        }

        let (expected_ordinal, event_count) = match current_commit {
            Some((commit_id, expected_ordinal, event_count)) if commit_id == event.commit_id() => {
                (expected_ordinal, event_count)
            }
            Some(_) => return Err(corrupt("loaded history contains an incomplete commit")),
            None => {
                if event.commit_event_ordinal() != 0 {
                    return Err(corrupt("loaded history starts inside a commit"));
                }
                if !seen_commits.insert(event.commit_id()) {
                    return Err(corrupt("events for one commit are not contiguous"));
                }
                (0, event.commit_event_count())
            }
        };
        if event.commit_event_ordinal() != expected_ordinal
            || event.commit_event_count() != event_count
        {
            return Err(corrupt(
                "loaded history has inconsistent commit coordinates",
            ));
        }
        if event.commit_id() != &derive_commit_id(stream_id, event.operation_id()) {
            return Err(corrupt(
                "loaded history contains an invalid commit identity",
            ));
        }
        if event.event_id() != &derive_event_id(event.commit_id(), event.commit_event_ordinal()) {
            return Err(corrupt("loaded history contains an invalid event identity"));
        }
        let next_ordinal = event
            .commit_event_ordinal()
            .checked_add(1)
            .ok_or_else(|| corrupt("a commit exceeds the supported event ordinal range"))?;
        current_commit = if next_ordinal == event_count {
            None
        } else {
            Some((event.commit_id(), next_ordinal, event_count))
        };

        let operation_identity = (event.commit_id(), event.operation_fingerprint());
        if let Some(previous) = operations.insert(event.operation_id(), operation_identity)
            && previous != operation_identity
        {
            return Err(corrupt(
                "loaded history reuses an operation identity across different commits",
            ));
        }
    }
    if current_commit.is_some() {
        return Err(corrupt("loaded history ended inside a commit"));
    }
    Ok(())
}

fn corrupt(message: &'static str) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::CorruptHistory, message)
}

fn invalid_request(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentFingerprint, EventCodecError, InMemoryEventStore, OperationId};

    const CONST_EXECUTOR: Executor<(), ()> =
        Executor::with_codec((), ()).with_max_conflict_retries(4);
    const DEFAULT_CONST_EXECUTOR: Executor<()> = Executor::new(());

    struct TestAggregate;

    impl Aggregate for TestAggregate {
        type State = ();
        type Event = TestEvent;

        const AGGREGATE_TYPE: &'static str = "test";

        fn initial(_stream_id: &StreamId) -> Self::State {}

        fn apply(_state: &mut Self::State, _event: &Self::Event) {}
    }

    struct TestEvent;

    impl Event for TestEvent {
        fn event_type(&self) -> &'static str {
            "test-event"
        }

        fn schema_version(&self) -> u32 {
            1
        }

        fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
            Ok(Vec::new())
        }

        fn decode_json(_event: &RecordedEvent) -> Result<Self, EventCodecError> {
            Ok(Self)
        }
    }

    fn metadata(aggregate_id: &str) -> ExecutionMetadata {
        ExecutionMetadata::new(
            StreamId::new(
                AggregateType::new(TestAggregate::AGGREGATE_TYPE).unwrap(),
                AggregateId::new(aggregate_id).unwrap(),
            ),
            OperationId::new("operation").unwrap(),
            ContentFingerprint::digest("command"),
        )
    }

    fn assert_invalid_request(error: CommandExecutionError) {
        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    #[test]
    fn executor_configuration_is_const_constructible() {
        assert_eq!(*CONST_EXECUTOR.store(), ());
        assert_eq!(*CONST_EXECUTOR.codec(), ());
        assert_eq!(*DEFAULT_CONST_EXECUTOR.store(), ());
    }

    #[tokio::test]
    async fn context_rejects_primary_and_duplicate_loads() {
        let store = InMemoryEventStore::new();
        let metadata = metadata("primary");
        let mut context = CommandContext::new(&store, &metadata);

        let Err(primary_error) = context.load::<TestAggregate>("primary").await else {
            panic!("primary load unexpectedly succeeded");
        };
        assert_invalid_request(primary_error);

        let _loaded = context.load::<TestAggregate>("secondary").await.unwrap();
        let Err(duplicate_error) = context.load::<TestAggregate>("secondary").await else {
            panic!("duplicate load unexpectedly succeeded");
        };
        assert_invalid_request(duplicate_error);
    }

    #[tokio::test]
    async fn context_rejects_loaded_aggregate_from_another_attempt() {
        let store = InMemoryEventStore::new();
        let metadata = metadata("primary");
        let mut first = CommandContext::new(&store, &metadata);
        let loaded = first.load::<TestAggregate>("secondary").await.unwrap();
        let mut second = CommandContext::new(&store, &metadata);

        assert_invalid_request(second.include(loaded).unwrap_err());
    }

    #[tokio::test]
    async fn context_preserves_an_included_zero_event_aggregate_as_a_read_guard() {
        let store = InMemoryEventStore::new();
        let metadata = metadata("primary");
        let mut context = CommandContext::new(&store, &metadata);
        let loaded = context.load::<TestAggregate>("secondary").await.unwrap();

        context.include(loaded).unwrap();
        let participants = context.finish().unwrap();

        assert_eq!(participants.len(), 1);
        assert!(participants[0].events.is_empty());
        assert_eq!(participants[0].base_version, StreamVersion::ZERO);
    }
}
