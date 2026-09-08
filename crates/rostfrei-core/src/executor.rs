use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use thiserror::Error;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::{
    Aggregate, AggregateId, AggregateInstance, AggregateType, AppendSession,
    CommandExecutionMetadata, Event, EventBatch, EventCodec, EventCodecError, EventHistory,
    EventStore, EventStoreError, EventStoreErrorKind, EventTransaction, ExpectedVersion,
    JsonEventCodec, NewEvent, RecordedEvent, StreamId, StreamVersion, TransactionAppendOutcome,
    TransactionParticipant, TransactionReceipt,
};

const DEFAULT_MAX_CONFLICT_RETRIES: usize = 3;

#[derive(Clone, Default)]
pub struct AggregateCodecs {
    codecs: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl AggregateCodecs {
    pub fn register<A, C>(&mut self, codec: C)
    where
        A: Aggregate + 'static,
        C: EventCodec<A> + 'static,
    {
        let codec: Arc<dyn EventCodec<A>> = Arc::new(codec);
        self.codecs.insert(TypeId::of::<A>(), Arc::new(codec));
    }

    fn resolve<A>(&self) -> Arc<dyn EventCodec<A>>
    where
        A: Aggregate + 'static,
        A::Event: Event,
    {
        self.codecs
            .get(&TypeId::of::<A>())
            .and_then(|codec| codec.downcast_ref::<Arc<dyn EventCodec<A>>>())
            .cloned()
            .unwrap_or_else(|| Arc::new(JsonEventCodec))
    }
}

pub struct CommandExecutor<S> {
    store: S,
    codecs: Arc<AggregateCodecs>,
    maximum_conflict_retries: usize,
}

impl<S> CommandExecutor<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            codecs: Arc::new(AggregateCodecs::default()),
            maximum_conflict_retries: DEFAULT_MAX_CONFLICT_RETRIES,
        }
    }

    pub const fn with_codecs(store: S, codecs: Arc<AggregateCodecs>) -> Self {
        Self {
            store,
            codecs,
            maximum_conflict_retries: DEFAULT_MAX_CONFLICT_RETRIES,
        }
    }

    /// Overrides the event codec used to load and persist one aggregate type.
    #[must_use]
    pub fn with_codec<A, C>(mut self, codec: C) -> Self
    where
        A: Aggregate + 'static,
        C: EventCodec<A> + 'static,
    {
        Arc::make_mut(&mut self.codecs).register::<A, C>(codec);
        self
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandDecision<Rejection> {
    Accepted,
    Rejected(Rejection),
}

pub type CommandHandlingResult<Rejection> =
    Result<CommandDecision<Rejection>, CommandExecutionError>;

/// Handles a command at the bounded-context application boundary.
#[async_trait]
pub trait CommandHandler<Command>: Send + Sync
where
    Command: Sync,
{
    type Rejection: Send;

    async fn handle(
        &self,
        command: &Command,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection>;
}

/// An aggregate loaded during one command-handling attempt.
pub struct LoadedAggregate<A: Aggregate> {
    aggregate: AggregateInstance<A>,
    base_version: StreamVersion,
    observer_token: Arc<()>,
    participant: Arc<Mutex<ParticipantTracker>>,
}

impl<A: Aggregate> LoadedAggregate<A> {
    pub const fn aggregate(&self) -> &AggregateInstance<A> {
        &self.aggregate
    }

    /// Returns the aggregate instance enlisted by this successful load.
    ///
    /// Event tracking follows this loaded instance. Aggregate instances constructed separately by
    /// application code are not enlisted automatically.
    pub const fn aggregate_mut(&mut self) -> &mut AggregateInstance<A> {
        &mut self.aggregate
    }

    pub const fn base_version(&self) -> StreamVersion {
        self.base_version
    }
}

impl<A: Aggregate> Drop for LoadedAggregate<A> {
    fn drop(&mut self) {
        let finalization = if self.aggregate.is_observed_by(&self.observer_token) {
            Ok(self.aggregate.uncommitted_events().len())
        } else {
            Err(aggregate_tracking_lost())
        };
        self.participant
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finalization = Some(finalization);
    }
}

enum JournalEntry {
    Started(usize),
    Completed(usize, Result<NewEvent, CommandExecutionError>),
}

struct ParticipantTracker {
    journal: Vec<JournalEntry>,
    finalization: Option<Result<usize, CommandExecutionError>>,
}

struct EnlistedParticipant {
    stream_id: StreamId,
    base_version: StreamVersion,
    tracker: Arc<Mutex<ParticipantTracker>>,
}

struct CollectedParticipant {
    stream_id: StreamId,
    base_version: StreamVersion,
    events: Vec<NewEvent>,
}

/// Per-attempt access to all aggregates participating in a command.
///
/// Each aggregate type uses its codec registered on [`CommandExecutor`], falling back to
/// [`JsonEventCodec`] when no override exists.
pub struct CommandExecution<'a> {
    history: &'a dyn EventHistory,
    metadata: &'a CommandExecutionMetadata,
    codecs: &'a AggregateCodecs,
    loaded_streams: HashSet<StreamId>,
    participants: Vec<EnlistedParticipant>,
}

impl<'a> CommandExecution<'a> {
    pub fn new(history: &'a dyn EventHistory, metadata: &'a CommandExecutionMetadata) -> Self {
        Self::with_codecs(history, metadata, default_codecs())
    }

    fn with_codecs(
        history: &'a dyn EventHistory,
        metadata: &'a CommandExecutionMetadata,
        codecs: &'a AggregateCodecs,
    ) -> Self {
        Self {
            history,
            metadata,
            codecs,
            loaded_streams: HashSet::new(),
            participants: Vec::new(),
        }
    }

    pub const fn metadata(&self) -> &CommandExecutionMetadata {
        self.metadata
    }

    pub async fn load<A>(
        &mut self,
        aggregate_id: &str,
    ) -> Result<LoadedAggregate<A>, CommandExecutionError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
    {
        validate_bounded_context::<A>(self.metadata)?;
        let stream_id = stream_id_for::<A>(aggregate_id)?;
        if self.loaded_streams.contains(&stream_id) {
            return Err(invalid_request(
                "an aggregate stream may be loaded only once per command attempt",
            )
            .into());
        }
        let history = self.history.load(&stream_id).await?;
        validate_history(&stream_id, &history)?;
        let codec = self.codecs.resolve::<A>();
        let events = history
            .iter()
            .map(|event| codec.decode(event))
            .collect::<Result<Vec<_>, _>>()?;
        let base_version = current_version(&history);
        let commit_id = derive_commit_id(&stream_id, self.metadata.operation_id());
        let tracker = Arc::new(Mutex::new(ParticipantTracker {
            journal: Vec::new(),
            finalization: None,
        }));
        let start_tracker = Arc::clone(&tracker);
        let completion_tracker = Arc::clone(&tracker);
        let event_codec = Arc::clone(&codec);
        let observer_token = Arc::new(());
        let mut aggregate = AggregateInstance::rehydrate(stream_id.clone(), events);
        aggregate.observe_raised_events(
            Arc::clone(&observer_token),
            move |ordinal| {
                start_tracker
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .journal
                    .push(JournalEntry::Started(ordinal));
            },
            move |ordinal, event| {
                let encoded = supported_ordinal(ordinal)
                    .map(|ordinal| derive_event_id(&commit_id, ordinal))
                    .map_err(CommandExecutionError::from)
                    .and_then(|event_id| {
                        event_codec
                            .encode(event, event_id)
                            .map_err(CommandExecutionError::from)
                    });
                completion_tracker
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .journal
                    .push(JournalEntry::Completed(ordinal, encoded));
            },
        );

        self.loaded_streams.insert(stream_id.clone());
        self.participants.push(EnlistedParticipant {
            stream_id,
            base_version,
            tracker: Arc::clone(&tracker),
        });
        Ok(LoadedAggregate {
            aggregate,
            base_version,
            observer_token,
            participant: tracker,
        })
    }

    fn finish(self) -> Result<Vec<CollectedParticipant>, CommandExecutionError> {
        self.participants
            .into_iter()
            .map(EnlistedParticipant::collect)
            .collect()
    }

    fn finish_rejected(self) -> Result<(), CommandExecutionError> {
        self.participants
            .into_iter()
            .try_for_each(|participant| participant.discard_events())
    }

    fn rejected_simulation_participants(
        self,
    ) -> Result<Vec<SimulatedParticipant>, CommandExecutionError> {
        self.participants
            .into_iter()
            .map(|participant| {
                participant.discard_events()?;
                Ok(SimulatedParticipant {
                    stream_id: participant.stream_id,
                    base_version: participant.base_version,
                    events: Vec::new(),
                    read_guard: true,
                })
            })
            .collect()
    }
}

impl EnlistedParticipant {
    fn collect(self) -> Result<CollectedParticipant, CommandExecutionError> {
        let mut tracker = self
            .tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let expected_event_count = tracker.take_finalization()?;

        let mut events = Vec::new();
        let mut encoding = None;
        for entry in tracker.journal.drain(..) {
            match entry {
                JournalEntry::Started(ordinal) if encoding.is_none() && ordinal == events.len() => {
                    encoding = Some(ordinal);
                }
                JournalEntry::Completed(ordinal, event) => {
                    let event = event?;
                    if encoding != Some(ordinal) {
                        return Err(invalid_event_journal());
                    }
                    events.push(event);
                    encoding = None;
                }
                JournalEntry::Started(_) => return Err(invalid_event_journal()),
            }
        }
        if encoding.is_some() {
            return Err(interrupted_event_encoding());
        }
        if events.len() != expected_event_count {
            return Err(invalid_event_journal());
        }
        drop(tracker);
        Ok(CollectedParticipant {
            stream_id: self.stream_id,
            base_version: self.base_version,
            events,
        })
    }

    fn discard_events(&self) -> Result<(), CommandExecutionError> {
        self.tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_finalization()?;
        Ok(())
    }
}

impl ParticipantTracker {
    fn take_finalization(&mut self) -> Result<usize, CommandExecutionError> {
        self.finalization
            .take()
            .ok_or_else(aggregate_outlived_handler)?
    }
}

fn default_codecs() -> &'static AggregateCodecs {
    static CODECS: std::sync::OnceLock<AggregateCodecs> = std::sync::OnceLock::new();
    CODECS.get_or_init(AggregateCodecs::default)
}

fn aggregate_outlived_handler() -> CommandExecutionError {
    invalid_request("a loaded aggregate escaped its command-handling attempt").into()
}

fn aggregate_tracking_lost() -> CommandExecutionError {
    invalid_request("a loaded aggregate replaced its automatically tracked instance").into()
}

fn invalid_event_journal() -> CommandExecutionError {
    invalid_request("a loaded aggregate produced an invalid event journal").into()
}

fn interrupted_event_encoding() -> CommandExecutionError {
    invalid_request("a loaded aggregate event did not finish encoding").into()
}

pub type CommandResult<Rejection> = Result<CommandOutcome<Rejection>, CommandExecutionError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome<Rejection> {
    Accepted(CommandReceipt),
    Rejected(Rejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandReceipt {
    Appended(Vec<RecordedEvent>),
    ExactReplay(Vec<RecordedEvent>),
    /// No aggregates were loaded, or every loaded aggregate produced no events.
    ///
    /// This result is not persisted as a durable idempotency receipt. Executing the same operation
    /// again may therefore invoke the command handler again.
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
    decision: CommandDecision<Rejection>,
    participants: Vec<SimulatedParticipant>,
}

impl<Rejection> SimulationOutcome<Rejection> {
    pub const fn decision(&self) -> &CommandDecision<Rejection> {
        &self.decision
    }

    pub fn participants(&self) -> &[SimulatedParticipant] {
        &self.participants
    }

    pub fn into_parts(self) -> (CommandDecision<Rejection>, Vec<SimulatedParticipant>) {
        (self.decision, self.participants)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SimulationError {
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(transparent)]
    Codec(#[from] EventCodecError),
}

impl From<CommandExecutionError> for SimulationError {
    fn from(error: CommandExecutionError) -> Self {
        match error {
            CommandExecutionError::Store(error) => Self::Store(error),
            CommandExecutionError::Codec(error) => Self::Codec(error),
        }
    }
}

impl<S> CommandExecutor<S>
where
    S: EventHistory,
{
    /// Rehydrates one aggregate with an explicitly supplied codec.
    ///
    /// Unlike the default and registered-codec path, this method does not require the aggregate
    /// event type to implement [`Event`].
    pub async fn rehydrate_with_codec<A, C>(
        &self,
        stream_id: &StreamId,
        codec: &C,
    ) -> Result<AggregateInstance<A>, SimulationError>
    where
        A: Aggregate,
        C: EventCodec<A>,
    {
        validate_aggregate_type::<A>(stream_id)?;
        let history = self.store.load(stream_id).await?;
        validate_history(stream_id, &history)?;
        let events = history
            .iter()
            .map(|event| codec.decode(event))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AggregateInstance::rehydrate(stream_id.clone(), events))
    }

    pub async fn rehydrate<A>(
        &self,
        stream_id: &StreamId,
    ) -> Result<AggregateInstance<A>, SimulationError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
    {
        validate_aggregate_type::<A>(stream_id)?;
        let history = self.store.load(stream_id).await?;
        validate_history(stream_id, &history)?;
        let codec = self.codecs.resolve::<A>();
        let events = history
            .iter()
            .map(|event| codec.decode(event))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AggregateInstance::rehydrate(stream_id.clone(), events))
    }

    pub async fn simulate<Handler, Command>(
        &self,
        handler: &Handler,
        metadata: CommandExecutionMetadata,
        command: &Command,
    ) -> Result<SimulationOutcome<Handler::Rejection>, SimulationError>
    where
        Handler: CommandHandler<Command>,
        Command: Sync,
    {
        let mut execution =
            CommandExecution::with_codecs(&self.store, &metadata, self.codecs.as_ref());
        let decision = handler.handle(command, &mut execution).await?;
        let participants = match decision {
            CommandDecision::Accepted => execution
                .finish()?
                .into_iter()
                .map(simulated_participant)
                .collect(),
            CommandDecision::Rejected(_) => execution.rejected_simulation_participants()?,
        };
        Ok(SimulationOutcome {
            decision,
            participants,
        })
    }
}

impl<S> CommandExecutor<S>
where
    S: EventStore,
{
    pub async fn execute<Handler, Command>(
        &self,
        handler: &Handler,
        metadata: CommandExecutionMetadata,
        command: &Command,
    ) -> CommandResult<Handler::Rejection>
    where
        Handler: CommandHandler<Command>,
        Command: Sync,
    {
        let mut remaining_conflict_retries = self.maximum_conflict_retries;
        loop {
            if let Some(receipt) = reconcile_existing(&self.store, &metadata).await? {
                return Ok(CommandOutcome::Accepted(receipt));
            }

            let session = self.store.append_session().await?;
            let mut execution =
                CommandExecution::with_codecs(session.as_ref(), &metadata, self.codecs.as_ref());
            match handler.handle(command, &mut execution).await? {
                CommandDecision::Rejected(rejection) => {
                    execution.finish_rejected()?;
                    if let Some(receipt) = reconcile_existing(&self.store, &metadata).await? {
                        return Ok(CommandOutcome::Accepted(receipt));
                    }
                    return Ok(CommandOutcome::Rejected(rejection));
                }
                CommandDecision::Accepted => {}
            }

            let participants = execution.finish()?;
            if !participants
                .iter()
                .any(|participant| !participant.events.is_empty())
            {
                if let Some(receipt) = reconcile_existing(&self.store, &metadata).await? {
                    return Ok(CommandOutcome::Accepted(receipt));
                }
                return Ok(CommandOutcome::Accepted(CommandReceipt::NoEvents));
            }
            match append_transaction(session, &metadata, participants).await? {
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

async fn append_transaction(
    session: Box<dyn AppendSession + '_>,
    metadata: &CommandExecutionMetadata,
    participants: Vec<CollectedParticipant>,
) -> Result<PersistenceAttempt, CommandExecutionError> {
    let participants = participants
        .into_iter()
        .map(|participant| {
            let batch =
                prepare_batch_for_stream(metadata, &participant.stream_id, participant.events)?;
            Ok(TransactionParticipant::new(
                participant.stream_id,
                expected_version(participant.base_version),
                batch,
            ))
        })
        .collect::<Result<Vec<_>, EventStoreError>>()?;
    let mut transaction = EventTransaction::new(
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        participants,
    )
    .with_bounded_context(required_bounded_context(metadata)?.clone());
    if let Some(correlation_id) = metadata.correlation_id() {
        transaction = transaction.with_correlation_id(correlation_id.clone());
    }
    if let Some(causation_id) = metadata.causation_id() {
        transaction = transaction.with_causation_id(causation_id.clone());
    }
    match session.append_transaction(transaction).await {
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
    metadata: &CommandExecutionMetadata,
) -> Result<Option<CommandReceipt>, CommandExecutionError> {
    store
        .load_transaction_receipt_in_context(
            required_bounded_context(metadata)?,
            metadata.operation_id(),
        )
        .await?
        .map(|receipt| transaction_replay(metadata, &receipt))
        .transpose()
}

fn transaction_replay(
    metadata: &CommandExecutionMetadata,
    receipt: &TransactionReceipt,
) -> Result<CommandReceipt, CommandExecutionError> {
    let identities_are_valid = receipt.streams().iter().all(|stream| {
        let commit_id = derive_commit_id(stream.stream_id(), receipt.operation_id());
        stream.events().iter().enumerate().all(|(ordinal, event)| {
            u32::try_from(ordinal).is_ok_and(|ordinal| {
                event.stream_id() == stream.stream_id()
                    && event.commit_id() == &commit_id
                    && event.event_id() == &derive_event_id(&commit_id, ordinal)
                    && event.operation_id() == receipt.operation_id()
                    && event.operation_fingerprint() == receipt.operation_fingerprint()
                    && event.correlation_id() == receipt.correlation_id()
                    && event.causation_id() == receipt.causation_id()
            })
        })
    });
    if receipt.bounded_context() == metadata.bounded_context()
        && receipt.operation_id() == metadata.operation_id()
        && receipt.operation_fingerprint() == metadata.operation_fingerprint()
        && receipt.correlation_id() == metadata.correlation_id()
        && receipt.causation_id() == metadata.causation_id()
        && identities_are_valid
    {
        return Ok(CommandReceipt::ExactReplay(receipt.events()));
    }
    Err(EventStoreError::new(
        EventStoreErrorKind::IdentityConflict,
        "operation identity was reused with different transaction metadata",
    )
    .into())
}

fn simulated_participant(participant: CollectedParticipant) -> SimulatedParticipant {
    SimulatedParticipant {
        stream_id: participant.stream_id,
        base_version: participant.base_version,
        read_guard: participant.events.is_empty(),
        events: participant.events,
    }
}

fn required_bounded_context(
    metadata: &CommandExecutionMetadata,
) -> Result<&rostfrei_messaging_core::BoundedContextName, EventStoreError> {
    metadata
        .bounded_context()
        .ok_or_else(|| invalid_request("command execution has no bounded-context identity"))
}

fn validate_bounded_context<A: Aggregate>(
    metadata: &CommandExecutionMetadata,
) -> Result<(), EventStoreError> {
    let execution_context = required_bounded_context(metadata)?;
    if A::BOUNDED_CONTEXT != execution_context.as_str() {
        return Err(invalid_request(format!(
            "aggregate context `{}` does not match command execution context `{}`",
            A::BOUNDED_CONTEXT,
            execution_context.as_str()
        )));
    }
    Ok(())
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
            "aggregate type {} cannot rehydrate stream type {}",
            aggregate_type,
            stream_id.aggregate_type()
        )));
    }
    Ok(())
}

fn prepare_batch_for_stream(
    metadata: &CommandExecutionMetadata,
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
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::{AppendOutcome, ContentFingerprint, InMemoryEventStore, OperationId};

    struct TestAggregate;

    impl Aggregate for TestAggregate {
        type State = usize;
        type Event = TestEvent;

        const BOUNDED_CONTEXT: &'static str = "test-context";
        const AGGREGATE_TYPE: &'static str = "test";

        fn initial(_stream_id: &StreamId) -> Self::State {
            0
        }

        fn apply(state: &mut Self::State, _event: &Self::Event) {
            *state = state.checked_add(1).expect("test event count overflowed");
        }
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

    fn metadata() -> CommandExecutionMetadata {
        CommandExecutionMetadata::new(
            OperationId::new("operation").unwrap(),
            ContentFingerprint::digest("command"),
        )
        .with_bounded_context(
            rostfrei_messaging_core::BoundedContextName::new("test-context").unwrap(),
        )
    }

    #[test]
    fn executor_configuration_preserves_store_and_retry_limit() {
        let executor = CommandExecutor::new(()).with_max_conflict_retries(4);
        assert_eq!(*executor.store(), ());
        assert_eq!(executor.maximum_conflict_retries, 4);
    }

    #[tokio::test]
    async fn successful_loads_are_automatically_enlisted() {
        let store = InMemoryEventStore::new();
        let metadata = metadata();
        let mut execution = CommandExecution::new(&store, &metadata);
        let mut first = execution.load::<TestAggregate>("first").await.unwrap();
        let mut second = execution.load::<TestAggregate>("second").await.unwrap();

        raise_on_both(first.aggregate_mut(), second.aggregate_mut());
        drop(first);
        drop(second);
        let participants = execution.finish().unwrap();

        assert_eq!(participants.len(), 2);
        assert!(
            participants
                .iter()
                .all(|participant| participant.events.len() == 1)
        );
    }

    fn raise_on_both(
        first: &mut AggregateInstance<TestAggregate>,
        second: &mut AggregateInstance<TestAggregate>,
    ) {
        first.raise(TestEvent);
        second.raise(TestEvent);
    }

    struct FailsFirstLoad {
        attempts: AtomicUsize,
    }

    #[async_trait]
    impl EventHistory for FailsFirstLoad {
        async fn load(&self, _stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
            if self.attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::Unavailable,
                    "injected load failure",
                ));
            }
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn failed_load_does_not_reserve_the_stream_in_the_unit_of_work() {
        let history = FailsFirstLoad {
            attempts: AtomicUsize::new(0),
        };
        let metadata = metadata();
        let mut execution = CommandExecution::new(&history, &metadata);

        let first = execution.load::<TestAggregate>("aggregate").await;
        assert!(matches!(
            first,
            Err(CommandExecutionError::Store(error))
                if error.kind() == EventStoreErrorKind::Unavailable
        ));
        let loaded = execution.load::<TestAggregate>("aggregate").await.unwrap();

        assert_eq!(loaded.base_version(), StreamVersion::ZERO);
        assert_eq!(history.attempts.load(Ordering::Relaxed), 2);
        drop(loaded);
        assert_eq!(execution.finish().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unit_of_work_rejects_duplicate_loads() {
        let store = InMemoryEventStore::new();
        let metadata = metadata();
        let mut execution = CommandExecution::new(&store, &metadata);
        let _loaded = execution.load::<TestAggregate>("aggregate").await.unwrap();

        let duplicate = execution.load::<TestAggregate>("aggregate").await;
        assert!(matches!(
            duplicate,
            Err(CommandExecutionError::Store(error))
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    struct WriteHandler {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl CommandHandler<()> for WriteHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let _guard = execution.load::<TestAggregate>("guard").await?;
            let mut writer = execution.load::<TestAggregate>("writer").await?;
            writer.aggregate_mut().raise(TestEvent);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn execution_commits_read_guard_first_and_replays_by_operation() {
        let store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(store.clone());
        let handler = WriteHandler {
            calls: AtomicUsize::new(0),
        };

        let first = executor.execute(&handler, metadata(), &()).await.unwrap();
        let second = executor.execute(&handler, metadata(), &()).await.unwrap();

        assert!(matches!(
            first,
            CommandOutcome::Accepted(CommandReceipt::Appended(_))
        ));
        assert!(matches!(
            second,
            CommandOutcome::Accepted(CommandReceipt::ExactReplay(_))
        ));
        assert_eq!(handler.calls.load(Ordering::Relaxed), 1);
        let receipt = store
            .load_transaction_receipt_in_context(
                metadata().bounded_context().unwrap(),
                metadata().operation_id(),
            )
            .await
            .unwrap()
            .unwrap();
        assert!(receipt.streams()[0].events().is_empty());
        assert_eq!(receipt.streams()[1].events().len(), 1);
    }

    struct EncodingFailureAggregate;

    impl Aggregate for EncodingFailureAggregate {
        type State = ();
        type Event = EncodingFailureEvent;

        const BOUNDED_CONTEXT: &'static str = "test-context";
        const AGGREGATE_TYPE: &'static str = "encoding-failure";

        fn initial(_stream_id: &StreamId) -> Self::State {}

        fn apply(_state: &mut Self::State, _event: &Self::Event) {}
    }

    struct EncodingFailureEvent;

    impl Event for EncodingFailureEvent {
        fn event_type(&self) -> &'static str {
            "encoding-failure"
        }

        fn schema_version(&self) -> u32 {
            1
        }

        fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
            Err(EventCodecError::new(
                crate::EventCodecErrorKind::EncodingFailed,
                "injected encoding failure",
            ))
        }

        fn decode_json(_event: &RecordedEvent) -> Result<Self, EventCodecError> {
            Ok(Self)
        }
    }

    struct EncodingFailureHandler;

    #[async_trait]
    impl CommandHandler<()> for EncodingFailureHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut successful = execution
                .load::<TestAggregate>("successful-aggregate")
                .await?;
            successful.aggregate_mut().raise(TestEvent);
            let mut failing = execution
                .load::<EncodingFailureAggregate>("failing-aggregate")
                .await?;
            failing.aggregate_mut().raise(EncodingFailureEvent);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn encoding_errors_surface_at_finish_without_a_partial_commit() {
        let store = InMemoryEventStore::new();

        let error = CommandExecutor::new(store.clone())
            .execute(&EncodingFailureHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(error, CommandExecutionError::Codec(_)));
        assert!(
            store
                .load(&stream_id_for::<TestAggregate>("successful-aggregate").unwrap())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .load(&stream_id_for::<EncodingFailureAggregate>("failing-aggregate").unwrap())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    struct PanickingEncodingAggregate;

    impl Aggregate for PanickingEncodingAggregate {
        type State = ();
        type Event = PanickingEncodingEvent;

        const BOUNDED_CONTEXT: &'static str = "test-context";
        const AGGREGATE_TYPE: &'static str = "panicking-encoding";

        fn initial(_stream_id: &StreamId) -> Self::State {}

        fn apply(_state: &mut Self::State, _event: &Self::Event) {}
    }

    struct PanickingEncodingEvent;

    impl Event for PanickingEncodingEvent {
        fn event_type(&self) -> &'static str {
            "panicking-encoding"
        }

        fn schema_version(&self) -> u32 {
            1
        }

        #[allow(
            clippy::panic_in_result_fn,
            reason = "the panic is the behavior exercised by the caught-encoding-panic test"
        )]
        fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
            panic!("caught encoding panic")
        }

        fn decode_json(_event: &RecordedEvent) -> Result<Self, EventCodecError> {
            Ok(Self)
        }
    }

    struct CatchesEncodingPanicHandler;

    #[async_trait]
    impl CommandHandler<()> for CatchesEncodingPanicHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut aggregate = execution
                .load::<PanickingEncodingAggregate>("aggregate")
                .await?;
            let _panic = catch_unwind(AssertUnwindSafe(|| {
                aggregate.aggregate_mut().raise(PanickingEncodingEvent);
            }));
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn caught_encoding_panic_leaves_an_incomplete_journal_and_fails_closed() {
        let store = InMemoryEventStore::new();

        let error = CommandExecutor::new(store.clone())
            .execute(&CatchesEncodingPanicHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
        assert!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    struct PanickingApplyAggregate;

    impl Aggregate for PanickingApplyAggregate {
        type State = ();
        type Event = PanickingApplyEvent;

        const BOUNDED_CONTEXT: &'static str = "test-context";
        const AGGREGATE_TYPE: &'static str = "panicking-apply";

        fn initial(_stream_id: &StreamId) -> Self::State {}

        fn apply(_state: &mut Self::State, _event: &Self::Event) {
            panic!("caught apply panic")
        }
    }

    struct PanickingApplyEvent;

    impl Event for PanickingApplyEvent {
        fn event_type(&self) -> &'static str {
            "panicking-apply"
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

    struct CatchesApplyPanicHandler;

    #[async_trait]
    impl CommandHandler<()> for CatchesApplyPanicHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut aggregate = execution
                .load::<PanickingApplyAggregate>("aggregate")
                .await?;
            let _panic = catch_unwind(AssertUnwindSafe(|| {
                aggregate.aggregate_mut().raise(PanickingApplyEvent);
            }));
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn caught_apply_panic_leaves_an_incomplete_journal_and_fails_closed() {
        let store = InMemoryEventStore::new();

        let error = CommandExecutor::new(store.clone())
            .execute(&CatchesApplyPanicHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
        assert!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    struct TwoWriterHandler;

    #[async_trait]
    impl CommandHandler<()> for TwoWriterHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut first = execution.load::<TestAggregate>("first-writer").await?;
            let mut second = execution.load::<TestAggregate>("second-writer").await?;
            raise_on_both(first.aggregate_mut(), second.aggregate_mut());
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn two_loaded_aggregates_are_committed_atomically() {
        let store = InMemoryEventStore::new();

        let outcome = CommandExecutor::new(store.clone())
            .execute(&TwoWriterHandler, metadata(), &())
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            CommandOutcome::Accepted(CommandReceipt::Appended(events)) if events.len() == 2
        ));
        let receipt = store
            .load_transaction_receipt_in_context(
                metadata().bounded_context().unwrap(),
                metadata().operation_id(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(receipt.streams().len(), 2);
        assert!(
            receipt
                .streams()
                .iter()
                .all(|stream| stream.events().len() == 1)
        );
    }

    #[derive(Clone)]
    struct ConflictOnceStore {
        inner: InMemoryEventStore,
        attempts: Arc<AtomicUsize>,
        sessions: Arc<AtomicUsize>,
        session_loads: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EventHistory for ConflictOnceStore {
        async fn load(&self, _stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
            Err(invalid_request(
                "execution must load through its append session",
            ))
        }
    }

    struct ConflictOnceSession<'a> {
        store: &'a ConflictOnceStore,
        session_id: usize,
        loads: AtomicUsize,
    }

    #[async_trait]
    impl EventHistory for ConflictOnceSession<'_> {
        async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
            self.loads.fetch_add(1, Ordering::Relaxed);
            self.store.session_loads.fetch_add(1, Ordering::Relaxed);
            self.store.inner.load(stream_id).await
        }
    }

    #[async_trait]
    impl AppendSession for ConflictOnceSession<'_> {
        async fn append(
            self: Box<Self>,
            stream_id: &StreamId,
            expected_version: ExpectedVersion,
            batch: EventBatch,
        ) -> Result<AppendOutcome, EventStoreError> {
            self.store.append(stream_id, expected_version, batch).await
        }

        async fn append_transaction(
            self: Box<Self>,
            transaction: EventTransaction,
        ) -> Result<TransactionAppendOutcome, EventStoreError> {
            assert_eq!(self.loads.load(Ordering::Relaxed), 2);
            assert_eq!(self.session_id, self.store.attempts.load(Ordering::Relaxed));
            self.store.append_transaction(transaction).await
        }
    }

    #[async_trait]
    impl EventStore for ConflictOnceStore {
        async fn append_session(&self) -> Result<Box<dyn AppendSession + '_>, EventStoreError> {
            Ok(Box::new(ConflictOnceSession {
                store: self,
                session_id: self.sessions.fetch_add(1, Ordering::Relaxed),
                loads: AtomicUsize::new(0),
            }))
        }

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
            operation_id: &OperationId,
        ) -> Result<Option<TransactionReceipt>, EventStoreError> {
            self.inner.load_transaction_receipt(operation_id).await
        }

        async fn load_transaction_receipt_in_context(
            &self,
            bounded_context: &rostfrei_messaging_core::BoundedContextName,
            operation_id: &OperationId,
        ) -> Result<Option<TransactionReceipt>, EventStoreError> {
            self.inner
                .load_transaction_receipt_in_context(bounded_context, operation_id)
                .await
        }

        async fn append_transaction(
            &self,
            transaction: EventTransaction,
        ) -> Result<TransactionAppendOutcome, EventStoreError> {
            if self.attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::Conflict,
                    "injected transaction conflict",
                ));
            }
            self.inner.append_transaction(transaction).await
        }
    }

    #[tokio::test]
    async fn a_conflict_retries_the_entire_handler_and_unit_of_work() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let sessions = Arc::new(AtomicUsize::new(0));
        let session_loads = Arc::new(AtomicUsize::new(0));
        let store: Arc<dyn EventStore> = Arc::new(ConflictOnceStore {
            inner: InMemoryEventStore::new(),
            attempts: Arc::clone(&attempts),
            sessions: Arc::clone(&sessions),
            session_loads: Arc::clone(&session_loads),
        });
        let handler = WriteHandler {
            calls: AtomicUsize::new(0),
        };

        let outcome = CommandExecutor::new(store)
            .execute(&handler, metadata(), &())
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            CommandOutcome::Accepted(CommandReceipt::Appended(_))
        ));
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert_eq!(sessions.load(Ordering::Relaxed), 2);
        assert_eq!(session_loads.load(Ordering::Relaxed), 4);
        assert_eq!(handler.calls.load(Ordering::Relaxed), 2);
    }

    struct ReceiptAfterDecisionStore {
        inner: InMemoryEventStore,
        receipt_lookups: AtomicUsize,
    }

    #[async_trait]
    impl EventHistory for ReceiptAfterDecisionStore {
        async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
            self.inner.load(stream_id).await
        }
    }

    #[async_trait]
    impl EventStore for ReceiptAfterDecisionStore {
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
            operation_id: &OperationId,
        ) -> Result<Option<TransactionReceipt>, EventStoreError> {
            if self.receipt_lookups.fetch_add(1, Ordering::Relaxed) == 0 {
                return Ok(None);
            }
            self.inner.load_transaction_receipt(operation_id).await
        }

        async fn load_transaction_receipt_in_context(
            &self,
            bounded_context: &rostfrei_messaging_core::BoundedContextName,
            operation_id: &OperationId,
        ) -> Result<Option<TransactionReceipt>, EventStoreError> {
            if self.receipt_lookups.fetch_add(1, Ordering::Relaxed) == 0 {
                return Ok(None);
            }
            self.inner
                .load_transaction_receipt_in_context(bounded_context, operation_id)
                .await
        }

        async fn append_transaction(
            &self,
            transaction: EventTransaction,
        ) -> Result<TransactionAppendOutcome, EventStoreError> {
            self.inner.append_transaction(transaction).await
        }
    }

    struct RejectingHandler;

    #[async_trait]
    impl CommandHandler<()> for RejectingHandler {
        type Rejection = &'static str;

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut aggregate = execution.load::<TestAggregate>("rejected").await?;
            aggregate.aggregate_mut().raise(TestEvent);
            Ok(CommandDecision::Rejected("rejected"))
        }
    }

    #[tokio::test]
    async fn rejection_discards_all_loaded_aggregate_events() {
        let store = InMemoryEventStore::new();
        let stream_id = stream_id_for::<TestAggregate>("rejected").unwrap();

        let outcome = CommandExecutor::new(store.clone())
            .execute(&RejectingHandler, metadata(), &())
            .await
            .unwrap();

        assert_eq!(outcome, CommandOutcome::Rejected("rejected"));
        assert!(store.load(&stream_id).await.unwrap().is_empty());
        assert!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn rejected_simulation_retains_touched_streams_but_discards_events() {
        let outcome = CommandExecutor::new(InMemoryEventStore::new())
            .simulate(&RejectingHandler, metadata(), &())
            .await
            .unwrap();

        assert_eq!(outcome.decision(), &CommandDecision::Rejected("rejected"));
        assert_eq!(outcome.participants().len(), 1);
        assert_eq!(
            outcome.participants()[0]
                .stream_id()
                .aggregate_id()
                .as_str(),
            "rejected"
        );
        assert!(outcome.participants()[0].events().is_empty());
        assert!(outcome.participants()[0].is_read_guard());
    }

    struct RejectedForgottenHandler;

    #[async_trait]
    impl CommandHandler<()> for RejectedForgottenHandler {
        type Rejection = &'static str;

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let aggregate = execution
                .load::<TestAggregate>("rejected-forgotten")
                .await?;
            std::mem::forget(aggregate);
            Ok(CommandDecision::Rejected("rejected"))
        }
    }

    #[tokio::test]
    async fn rejected_execution_fails_closed_when_a_loaded_aggregate_is_forgotten() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&RejectedForgottenHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    #[tokio::test]
    async fn rejected_simulation_fails_closed_when_a_loaded_aggregate_is_forgotten() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .simulate(&RejectedForgottenHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            SimulationError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    struct RejectedReplacingHandler;

    #[async_trait]
    impl CommandHandler<()> for RejectedReplacingHandler {
        type Rejection = &'static str;

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut loaded = execution.load::<TestAggregate>("rejected-replaced").await?;
            let stream_id = loaded.aggregate().stream_id().clone();
            *loaded.aggregate_mut() = AggregateInstance::new(stream_id);
            Ok(CommandDecision::Rejected("rejected"))
        }
    }

    #[tokio::test]
    async fn rejected_execution_fails_closed_when_the_tracked_instance_is_replaced() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&RejectedReplacingHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    #[tokio::test]
    async fn rejected_simulation_fails_closed_when_the_tracked_instance_is_replaced() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .simulate(&RejectedReplacingHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            SimulationError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    struct OtherContextAggregate;

    impl Aggregate for OtherContextAggregate {
        type State = usize;
        type Event = TestEvent;

        const BOUNDED_CONTEXT: &'static str = "other-context";
        const AGGREGATE_TYPE: &'static str = "other";

        fn initial(_stream_id: &StreamId) -> Self::State {
            0
        }

        fn apply(state: &mut Self::State, _event: &Self::Event) {
            *state = state.saturating_add(1);
        }
    }

    struct OtherContextHandler;

    #[async_trait]
    impl CommandHandler<()> for OtherContextHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut aggregate = execution.load::<OtherContextAggregate>("other").await?;
            aggregate.aggregate_mut().raise(TestEvent);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn execution_rejects_aggregates_from_another_bounded_context() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&OtherContextHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
                    && error.message().contains("other-context")
                    && error.message().contains("test-context")
        ));
    }

    #[tokio::test]
    async fn transaction_operation_identity_is_scoped_by_bounded_context() {
        let store = InMemoryEventStore::new();
        let first = CommandExecutor::new(store.clone())
            .execute(
                &WriteHandler {
                    calls: AtomicUsize::new(0),
                },
                metadata(),
                &(),
            )
            .await
            .unwrap();
        let other_context =
            rostfrei_messaging_core::BoundedContextName::new("other-context").unwrap();
        let other_metadata = CommandExecutionMetadata::new(
            metadata().operation_id().clone(),
            metadata().operation_fingerprint(),
        )
        .with_bounded_context(other_context.clone());
        let second = CommandExecutor::new(store.clone())
            .execute(&OtherContextHandler, other_metadata, &())
            .await
            .unwrap();

        assert!(matches!(
            first,
            CommandOutcome::Accepted(CommandReceipt::Appended(_))
        ));
        assert!(matches!(
            second,
            CommandOutcome::Accepted(CommandReceipt::Appended(_))
        ));
        assert!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id(),
                )
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .load_transaction_receipt_in_context(&other_context, metadata().operation_id(),)
                .await
                .unwrap()
                .is_some()
        );
    }

    struct ForgottenHandler;

    #[async_trait]
    impl CommandHandler<()> for ForgottenHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let aggregate = execution.load::<TestAggregate>("escaped").await?;
            std::mem::forget(aggregate);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn accepted_execution_fails_closed_when_a_loaded_aggregate_is_forgotten() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&ForgottenHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    struct EscapingHandler {
        aggregate: Mutex<Option<LoadedAggregate<TestAggregate>>>,
    }

    #[async_trait]
    impl CommandHandler<()> for EscapingHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let aggregate = execution.load::<TestAggregate>("escaped").await?;
            *self.aggregate.lock().unwrap() = Some(aggregate);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn accepted_execution_fails_closed_when_a_loaded_aggregate_escapes() {
        let handler = EscapingHandler {
            aggregate: Mutex::new(None),
        };

        let error = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&handler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    struct ReplacingHandler;

    #[async_trait]
    impl CommandHandler<()> for ReplacingHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut loaded = execution.load::<TestAggregate>("replaced").await?;
            let stream_id = loaded.aggregate().stream_id().clone();
            *loaded.aggregate_mut() = AggregateInstance::new(stream_id);
            loaded.aggregate_mut().raise(TestEvent);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn accepted_execution_fails_closed_when_the_tracked_instance_is_replaced() {
        let store = InMemoryEventStore::new();
        let stream_id = stream_id_for::<TestAggregate>("replaced").unwrap();

        let error = CommandExecutor::new(store.clone())
            .execute(&ReplacingHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
        assert!(store.load(&stream_id).await.unwrap().is_empty());
    }

    struct PermanentSwapHandler;

    #[async_trait]
    impl CommandHandler<()> for PermanentSwapHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut first = execution.load::<TestAggregate>("swap-first").await?;
            let mut second = execution.load::<TestAggregate>("swap-second").await?;
            std::mem::swap(first.aggregate_mut(), second.aggregate_mut());
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn accepted_execution_fails_closed_when_tracked_instances_are_swapped() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&PermanentSwapHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    struct SwapBackHandler;

    #[async_trait]
    impl CommandHandler<()> for SwapBackHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut first = execution.load::<TestAggregate>("swap-back-first").await?;
            let mut second = execution.load::<TestAggregate>("swap-back-second").await?;
            std::mem::swap(first.aggregate_mut(), second.aggregate_mut());
            raise_on_both(first.aggregate_mut(), second.aggregate_mut());
            std::mem::swap(first.aggregate_mut(), second.aggregate_mut());
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn swapping_tracked_instances_back_restores_valid_tracking() {
        let outcome = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&SwapBackHandler, metadata(), &())
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            CommandOutcome::Accepted(CommandReceipt::Appended(events)) if events.len() == 2
        ));
    }

    struct CaughtPanicAfterReplacementHandler;

    #[async_trait]
    impl CommandHandler<()> for CaughtPanicAfterReplacementHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let mut loaded = execution.load::<TestAggregate>("panic-replaced").await?;
            let stream_id = loaded.aggregate().stream_id().clone();
            let _panic = catch_unwind(AssertUnwindSafe(|| {
                *loaded.aggregate_mut() = AggregateInstance::new(stream_id);
                panic!("caught replacement panic");
            }));
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn caught_panic_cannot_bypass_tracked_instance_validation() {
        let error = CommandExecutor::new(InMemoryEventStore::new())
            .execute(&CaughtPanicAfterReplacementHandler, metadata(), &())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest
        ));
    }

    struct NoEventsHandler;

    #[async_trait]
    impl CommandHandler<()> for NoEventsHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            let _aggregate = execution.load::<TestAggregate>("guard-only").await?;
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn automatically_enlisted_read_guards_without_writes_return_no_events() {
        let store = InMemoryEventStore::new();
        let outcome = CommandExecutor::new(store.clone())
            .execute(&NoEventsHandler, metadata(), &())
            .await
            .unwrap();

        assert_eq!(outcome, CommandOutcome::Accepted(CommandReceipt::NoEvents));
        assert!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn no_event_decision_reconciles_a_receipt_committed_while_handling() {
        let inner = InMemoryEventStore::new();
        let metadata = metadata();
        let stream_id = stream_id_for::<TestAggregate>("concurrent-writer").unwrap();
        let commit_id = derive_commit_id(&stream_id, metadata.operation_id());
        let batch = EventBatch::new(
            commit_id.clone(),
            metadata.operation_id().clone(),
            metadata.operation_fingerprint(),
            vec![
                NewEvent::new(derive_event_id(&commit_id, 0), "test-event", 1, Vec::new()).unwrap(),
            ],
        )
        .unwrap();
        inner
            .append_transaction(
                EventTransaction::new(
                    metadata.operation_id().clone(),
                    metadata.operation_fingerprint(),
                    vec![TransactionParticipant::new(
                        stream_id,
                        ExpectedVersion::NoStream,
                        Some(batch),
                    )],
                )
                .with_bounded_context(metadata.bounded_context().unwrap().clone()),
            )
            .await
            .unwrap();
        let store = ReceiptAfterDecisionStore {
            inner,
            receipt_lookups: AtomicUsize::new(0),
        };

        let outcome = CommandExecutor::new(store)
            .execute(&NoEventsHandler, metadata, &())
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            CommandOutcome::Accepted(CommandReceipt::ExactReplay(events)) if events.len() == 1
        ));
    }
}
