use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use thiserror::Error;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::{
    Aggregate, AggregateCodecs, AggregateId, AggregateInstance, AggregateType,
    CommandCommitSession, CommandExecutionMetadata, Event, EventBatch, EventCodec, EventCodecError,
    EventHistory, EventStoreError, EventStoreErrorKind, EventTransaction, ExecutionStore,
    ExpectedVersion, NewEvent, RecordedEvent, StreamId, StreamVersion, TransactionParticipant,
    TransactionReceipt,
};

pub struct CommandExecutor<S> {
    store: S,
    codecs: Arc<AggregateCodecs>,
    shared_codecs: bool,
    conflict_retry_policy: Arc<dyn crate::ConflictRetryPolicy>,
}

impl<S> CommandExecutor<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            codecs: Arc::new(AggregateCodecs::default()),
            shared_codecs: false,
            conflict_retry_policy: Arc::new(crate::BoundedConflictRetries::default()),
        }
    }

    pub fn with_codecs(store: S, codecs: Arc<AggregateCodecs>) -> Self {
        Self {
            store,
            codecs,
            shared_codecs: true,
            conflict_retry_policy: Arc::new(crate::BoundedConflictRetries::default()),
        }
    }

    /// Overrides the event codec used to load and persist one aggregate type.
    /// Standalone executors allow explicit replacement. Once `with_codecs` selects
    /// a shared effective registry, local overrides are rejected, even if uniquely owned.
    pub fn with_codec<A, C>(mut self, codec: C) -> Result<Self, crate::CodecRegistrationError>
    where
        A: Aggregate + 'static,
        C: EventCodec<A> + 'static,
    {
        if self.shared_codecs {
            return Err(crate::CodecRegistrationError::SharedConfiguration);
        }
        Arc::make_mut(&mut self.codecs).replace::<A, C>(codec)?;
        Ok(self)
    }

    #[must_use]
    pub fn with_max_conflict_retries(self, maximum_conflict_retries: usize) -> Self {
        self.with_conflict_retry_policy(Arc::new(crate::BoundedConflictRetries::new(
            maximum_conflict_retries,
        )))
    }

    #[must_use]
    pub fn with_conflict_retry_policy(
        mut self,
        policy: Arc<dyn crate::ConflictRetryPolicy>,
    ) -> Self {
        self.conflict_retry_policy = policy;
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
///
/// Dereferences to its tracked [`AggregateInstance`], so domain action traits in scope can be
/// invoked directly on the loaded handle. A mutable binding is sufficient to call mutable
/// actions; event tracking and finalization remain attached to the loaded instance.
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

impl<A: Aggregate> Deref for LoadedAggregate<A> {
    type Target = AggregateInstance<A>;

    fn deref(&self) -> &Self::Target {
        &self.aggregate
    }
}

impl<A: Aggregate> DerefMut for LoadedAggregate<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.aggregate
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
/// [`crate::JsonEventCodec`] when no override exists.
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
        let codec = self.codecs.resolve::<A>()?;
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

    fn finish_rejected(self) -> Result<Vec<CollectedParticipant>, CommandExecutionError> {
        self.participants
            .into_iter()
            .map(|participant| {
                participant.discard_events()?;
                Ok(CollectedParticipant {
                    stream_id: participant.stream_id,
                    base_version: participant.base_version,
                    events: Vec::new(),
                })
            })
            .collect()
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
    /// Newly persisted domain events and their durable acceptance receipt.
    Appended(Vec<RecordedEvent>),
    /// The original durable acceptance, without invoking the handler again on retry.
    /// An empty event list replays an accepted event-free command.
    ExactReplay(Vec<RecordedEvent>),
    /// Durable acceptance without domain events, including commands that load no aggregates.
    ///
    /// All loaded aggregate versions were guarded atomically with the receipt write.
    /// Retries return `ExactReplay` with an empty event list. Simulation is the read-only,
    /// transient alternative; execution never reports acceptance without persistence.
    AcceptedNoEvents,
}

impl CommandReceipt {
    pub fn events(&self) -> &[RecordedEvent] {
        match self {
            Self::Appended(events) | Self::ExactReplay(events) => events,
            Self::AcceptedNoEvents => &[],
        }
    }

    pub const fn is_exact_replay(&self) -> bool {
        matches!(self, Self::ExactReplay(_))
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CommandExecutionError {
    #[error(transparent)]
    OutcomeRecovery(#[from] crate::OutcomeRecoveryError),
    #[error("rejected outcome persistence failed: {0}")]
    RejectedOutcomePersistence(EventStoreError),
    #[error("rejection mapping failed: {0}")]
    RejectionMapping(String),
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(transparent)]
    Codec(#[from] EventCodecError),
}

impl CommandExecutionError {
    /// Technical classification for diagnostics. Request/identity failures and deliberate
    /// expiry remain separate; callers must inspect the typed outcome-recovery error.
    pub const fn execution_failure(&self) -> Option<rostfrei_messaging_core::ExecutionFailure> {
        use crate::EventCodecErrorKind;
        use rostfrei_messaging_core::{CodecFailureKind, ExecutionFailure, StoreFailureKind};
        Some(match self {
            Self::OutcomeRecovery(error) => ExecutionFailure::Store(match error {
                crate::OutcomeRecoveryError::Unavailable {
                    reason: crate::DetailUnavailableReason::BackendUnavailable,
                }
                | crate::OutcomeRecoveryError::Archived { .. } => StoreFailureKind::Unavailable,
                crate::OutcomeRecoveryError::Expired => return None,
                crate::OutcomeRecoveryError::IntegrityMismatch
                | crate::OutcomeRecoveryError::Unavailable {
                    reason: crate::DetailUnavailableReason::MissingDetails,
                } => StoreFailureKind::CorruptHistory,
            }),
            Self::RejectionMapping(_) => ExecutionFailure::Processor(
                rostfrei_messaging_core::ProcessorFailureKind::InvalidConfiguration,
            ),
            Self::Store(error) | Self::RejectedOutcomePersistence(error) => {
                ExecutionFailure::Store(match error.kind() {
                    EventStoreErrorKind::CorruptHistory => StoreFailureKind::CorruptHistory,
                    EventStoreErrorKind::CapacityExhausted => StoreFailureKind::CapacityExhausted,
                    EventStoreErrorKind::ConfigurationMismatch
                    | EventStoreErrorKind::DetailForbidden => {
                        StoreFailureKind::ConfigurationMismatch
                    }
                    EventStoreErrorKind::Conflict => StoreFailureKind::Conflict,
                    EventStoreErrorKind::Unavailable
                    | EventStoreErrorKind::CompletedDetailsUnavailable => {
                        StoreFailureKind::Unavailable
                    }
                    EventStoreErrorKind::InvalidRequest
                    | EventStoreErrorKind::IdentityConflict
                    | EventStoreErrorKind::CompletedDetailsExpired => {
                        return None;
                    }
                })
            }
            Self::Codec(error) => ExecutionFailure::Codec(match error.kind() {
                EventCodecErrorKind::UnknownEventType => CodecFailureKind::UnknownEventType,
                EventCodecErrorKind::UnsupportedSchemaVersion => {
                    CodecFailureKind::UnsupportedSchemaVersion
                }
                EventCodecErrorKind::MalformedPayload => CodecFailureKind::MalformedPayload,
                EventCodecErrorKind::InvalidEnvelope => CodecFailureKind::InvalidEnvelope,
                EventCodecErrorKind::EncodingFailed => CodecFailureKind::EncodingFailed,
            }),
        })
    }
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
    OutcomeRecovery(#[from] crate::OutcomeRecoveryError),
    #[error("rejection mapping failed: {0}")]
    RejectionMapping(String),
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(transparent)]
    Codec(#[from] EventCodecError),
}

impl From<CommandExecutionError> for SimulationError {
    fn from(error: CommandExecutionError) -> Self {
        match error {
            CommandExecutionError::OutcomeRecovery(error) => Self::OutcomeRecovery(error),
            CommandExecutionError::RejectionMapping(message) => Self::RejectionMapping(message),
            CommandExecutionError::Store(error)
            | CommandExecutionError::RejectedOutcomePersistence(error) => Self::Store(error),
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
        let codec = self.codecs.resolve::<A>()?;
        let events = history
            .iter()
            .map(|event| codec.decode(event))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AggregateInstance::rehydrate(stream_id.clone(), events))
    }

    /// Runs a handler without persistence, applying the same metadata defaults as execution.
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
        let metadata = metadata
            .with_default_outcome_evidence::<Command>()
            .normalized()
            .map_err(|error| invalid_request(error.to_string()))?;
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
    S: ExecutionStore,
{
    pub async fn reconcile_with_rejection_codec<R>(
        &self,
        metadata: CommandExecutionMetadata,
        codec: &dyn crate::RejectionCodec<R>,
    ) -> Result<Option<CommandOutcome<R>>, CommandExecutionError> {
        let metadata = metadata
            .normalized()
            .map_err(|error| invalid_request(error.to_string()))?;
        reconcile_completed(&self.store, &metadata, codec).await
    }
    /// Executes a command and atomically persists its accepted events.
    ///
    /// Before receipt lookup or handler invocation, an omitted correlation ID defaults to
    /// the operation ID. Explicit correlation and causation are preserved independently.
    /// The handler, transaction receipt, and committed events share this normalized metadata,
    /// which stays stable across conflict retries and exact replay. An operation ID that cannot
    /// serve as the default correlation ID fails with [`EventStoreErrorKind::InvalidRequest`]
    /// before execution begins.
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
        self.execute_with_rejection_codec(
            handler,
            metadata,
            command,
            &crate::completed_outcome::UnconfiguredRejectionCodec,
        )
        .await
    }

    /// Executes against the shared durable authority using an explicit rejection codec.
    /// The compatibility `execute` entry point fails closed if a rejection needs a codec.
    pub async fn execute_with_rejection_codec<Handler, Command>(
        &self,
        handler: &Handler,
        metadata: CommandExecutionMetadata,
        command: &Command,
        codec: &dyn crate::RejectionCodec<Handler::Rejection>,
    ) -> CommandResult<Handler::Rejection>
    where
        Handler: CommandHandler<Command>,
        Command: Sync,
    {
        let metadata = metadata
            .with_default_outcome_evidence::<Command>()
            .normalized()
            .map_err(|error| invalid_request(error.to_string()))?;
        let completed = self
            .execute_completed_with_rejection_codec(handler, metadata.clone(), command, codec)
            .await?;
        completed_replay(
            &metadata,
            completed.outcome(),
            codec,
            completed.is_exact_replay(),
        )
    }

    /// Executes through the same guarded loop, preserving the authoritative marker,
    /// availability and commit/replay origin for processing services.
    pub async fn execute_completed_with_rejection_codec<Handler, Command>(
        &self,
        handler: &Handler,
        metadata: CommandExecutionMetadata,
        command: &Command,
        codec: &dyn crate::RejectionCodec<Handler::Rejection>,
    ) -> Result<crate::CompletedCommit, CommandExecutionError>
    where
        Handler: CommandHandler<Command>,
        Command: Sync,
    {
        let metadata = metadata
            .with_default_outcome_evidence::<Command>()
            .normalized()
            .map_err(|error| invalid_request(error.to_string()))?;
        if let Some(evidence) = metadata.outcome_evidence() {
            evidence.validate()?;
        }
        let mut evaluations = 0_usize;
        loop {
            if let Some(outcome) = lookup_completed(&self.store, &metadata).await? {
                return Ok(crate::CompletedCommit::ExactReplay(outcome));
            }

            let session = self.store.begin_command().await?;
            evaluations = evaluations.saturating_add(1);
            let mut execution =
                CommandExecution::with_codecs(session.as_ref(), &metadata, self.codecs.as_ref());
            let (participants, rejection) = match handler.handle(command, &mut execution).await? {
                CommandDecision::Rejected(rejection) => {
                    // Finalize tracking and consume the original guarded session before
                    // reconciling a winner. A concurrent decision cannot excuse invalid
                    // handles or an observed loss of this attempt's authority incarnation.
                    let participants = execution.finish_rejected()?;
                    let encoded = codec.encode(&rejection)?;
                    encoded.validate()?;
                    // A codec must be able to replay its own output before it becomes authority.
                    drop(codec.decode(&encoded)?);
                    (participants, Some(encoded))
                }
                CommandDecision::Accepted => (execution.finish()?, None),
            };

            match append_transaction(session, &metadata, participants, rejection).await? {
                PersistenceAttempt::Completed(outcome) => {
                    let key = crate::OutcomeKey::new(
                        self.store.authority().clone(),
                        required_bounded_context(&metadata)?.clone(),
                        metadata.operation_id().clone(),
                    );
                    outcome
                        .outcome()
                        .validate_for(&key, &metadata)
                        .map_err(EventStoreError::in_outcome_recovery)?;
                    return Ok(*outcome);
                }
                PersistenceAttempt::Conflict(error) => {
                    // A simultaneous execution can decide differently against newer state.
                    // The winning acceptance is authoritative if the command evidence matches.
                    if let Some(outcome) = lookup_completed(&self.store, &metadata).await? {
                        return Ok(crate::CompletedCommit::ExactReplay(outcome));
                    }
                    if error.kind() != EventStoreErrorKind::Conflict {
                        return Err(error.into());
                    }
                    match self
                        .conflict_retry_policy
                        .after_conflict(crate::ConflictRetry::new(evaluations, error.clone()))
                    {
                        crate::ConflictRetryDecision::Stop => return Err(error.into()),
                        crate::ConflictRetryDecision::RetryNow => {}
                        crate::ConflictRetryDecision::RetryAfter(delay) => {
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
            }
        }
    }
}

enum PersistenceAttempt {
    Completed(Box<crate::CompletedCommit>),
    Conflict(EventStoreError),
}

async fn append_transaction(
    session: Box<dyn CommandCommitSession + '_>,
    metadata: &CommandExecutionMetadata,
    participants: Vec<CollectedParticipant>,
    rejection: Option<crate::EncodedRejection>,
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
    if let Some(scope) = metadata.security_scope() {
        transaction = transaction.with_security_scope(scope.clone());
    }
    let is_rejected = rejection.is_some();
    transaction = transaction.with_completion(crate::CommandCompletion::new(
        metadata.outcome_evidence().cloned(),
        rejection,
    ));
    match session.commit_outcome(transaction).await {
        Ok(outcome) => Ok(PersistenceAttempt::Completed(Box::new(outcome))),
        Err(error)
            if matches!(
                error.kind(),
                EventStoreErrorKind::Conflict | EventStoreErrorKind::IdentityConflict
            ) =>
        {
            Ok(PersistenceAttempt::Conflict(error))
        }
        Err(error) if error.origin() == crate::EventStoreErrorOrigin::OutcomeRecovery => {
            Err(error.into())
        }
        Err(error) if is_rejected => Err(CommandExecutionError::RejectedOutcomePersistence(error)),
        Err(error) => Err(error.into()),
    }
}

async fn reconcile_completed<S: ExecutionStore, R>(
    store: &S,
    metadata: &CommandExecutionMetadata,
    codec: &dyn crate::RejectionCodec<R>,
) -> Result<Option<CommandOutcome<R>>, CommandExecutionError> {
    lookup_completed(store, metadata)
        .await?
        .map(|outcome| completed_replay(metadata, &outcome, codec, true))
        .transpose()
}

async fn lookup_completed<S: ExecutionStore>(
    store: &S,
    metadata: &CommandExecutionMetadata,
) -> Result<Option<crate::CompletedOutcome>, CommandExecutionError> {
    let key = crate::OutcomeKey::new(
        store.authority().clone(),
        required_bounded_context(metadata)?.clone(),
        metadata.operation_id().clone(),
    );
    match store
        .lookup_outcome(&key)
        .await
        .map_err(EventStoreError::in_outcome_recovery)?
    {
        crate::OutcomeLookup::Missing => Ok(None),
        crate::OutcomeLookup::Completed(outcome) => {
            outcome
                .validate_for(&key, metadata)
                .map_err(EventStoreError::in_outcome_recovery)?;
            Ok(Some(outcome))
        }
    }
}

fn completed_replay<R>(
    metadata: &CommandExecutionMetadata,
    outcome: &crate::CompletedOutcome,
    codec: &dyn crate::RejectionCodec<R>,
    replay: bool,
) -> CommandResult<R> {
    outcome.marker().validate_identity(metadata)?;
    let inline = outcome.to_inline()?;
    let record = inline.record();
    let receipt = transaction_replay(metadata, record)?;
    if let Some(completion) = record.completion() {
        completion.validate()?;
        if completion.rejection.is_some() && !record.events().is_empty() {
            return Err(EventCodecError::new(
                crate::EventCodecErrorKind::InvalidEnvelope,
                "rejected record contains domain events",
            )
            .into());
        }
        let same_evidence = match (completion.evidence.as_ref(), metadata.outcome_evidence()) {
            (Some(recorded), Some(requested)) => recorded.same_identity(requested),
            (None, None) => true,
            _ => false,
        };
        if !same_evidence {
            return Err(EventStoreError::new(
                EventStoreErrorKind::IdentityConflict,
                "completed command evidence differs",
            )
            .into());
        }
    }
    if let Some(rejection) = inline.rejection() {
        return Ok(CommandOutcome::Rejected(codec.decode(rejection)?));
    }
    Ok(CommandOutcome::Accepted(if replay {
        receipt
    } else if record.events().is_empty() {
        CommandReceipt::AcceptedNoEvents
    } else {
        CommandReceipt::Appended(record.events())
    }))
}

fn transaction_replay(
    metadata: &CommandExecutionMetadata,
    receipt: &TransactionReceipt,
) -> Result<CommandReceipt, CommandExecutionError> {
    receipt.validate_integrity()?;
    if receipt.bounded_context() == metadata.bounded_context()
        && receipt.operation_id() == metadata.operation_id()
        && receipt.operation_fingerprint() == metadata.operation_fingerprint()
        && receipt.correlation_id() == metadata.correlation_id()
        && receipt.causation_id() == metadata.causation_id()
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
    use crate::{CompletedCommit, EventStore, OutcomeKey, OutcomeLookup, OutcomeReader};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::{ContentFingerprint, InMemoryEventStore, OperationId};

    #[test]
    fn all_execution_source_kinds_have_explicit_lossless_technical_policy() {
        use rostfrei_messaging_core::{CodecFailureKind, ExecutionFailure, StoreFailureKind};
        for (source, expected) in [
            (
                EventStoreErrorKind::CorruptHistory,
                StoreFailureKind::CorruptHistory,
            ),
            (
                EventStoreErrorKind::CapacityExhausted,
                StoreFailureKind::CapacityExhausted,
            ),
            (
                EventStoreErrorKind::ConfigurationMismatch,
                StoreFailureKind::ConfigurationMismatch,
            ),
            (EventStoreErrorKind::Conflict, StoreFailureKind::Conflict),
            (
                EventStoreErrorKind::Unavailable,
                StoreFailureKind::Unavailable,
            ),
        ] {
            let error = CommandExecutionError::Store(EventStoreError::new(source, "not parsed"));
            assert_eq!(
                error.execution_failure(),
                Some(ExecutionFailure::Store(expected))
            );
            assert_eq!(
                error.execution_failure().unwrap().is_retryable(),
                matches!(
                    source,
                    EventStoreErrorKind::Conflict | EventStoreErrorKind::Unavailable
                )
            );
        }
        for source in [
            EventStoreErrorKind::InvalidRequest,
            EventStoreErrorKind::IdentityConflict,
        ] {
            assert_eq!(
                CommandExecutionError::Store(EventStoreError::new(source, "not parsed"))
                    .execution_failure(),
                None
            );
        }
        for (source, expected) in [
            (
                crate::EventCodecErrorKind::UnknownEventType,
                CodecFailureKind::UnknownEventType,
            ),
            (
                crate::EventCodecErrorKind::UnsupportedSchemaVersion,
                CodecFailureKind::UnsupportedSchemaVersion,
            ),
            (
                crate::EventCodecErrorKind::MalformedPayload,
                CodecFailureKind::MalformedPayload,
            ),
            (
                crate::EventCodecErrorKind::InvalidEnvelope,
                CodecFailureKind::InvalidEnvelope,
            ),
            (
                crate::EventCodecErrorKind::EncodingFailed,
                CodecFailureKind::EncodingFailed,
            ),
        ] {
            let error = CommandExecutionError::Codec(EventCodecError::new(source, "not parsed"));
            assert_eq!(
                error.execution_failure(),
                Some(ExecutionFailure::Codec(expected))
            );
            assert!(!error.execution_failure().unwrap().is_retryable());
        }
    }

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
        assert_eq!(
            executor
                .conflict_retry_policy
                .after_conflict(crate::ConflictRetry::new(
                    4,
                    EventStoreError::new(EventStoreErrorKind::Conflict, "test")
                )),
            crate::ConflictRetryDecision::RetryNow
        );
        assert_eq!(
            executor
                .conflict_retry_policy
                .after_conflict(crate::ConflictRetry::new(
                    5,
                    EventStoreError::new(EventStoreErrorKind::Conflict, "test")
                )),
            crate::ConflictRetryDecision::Stop
        );
    }

    #[tokio::test]
    async fn successful_loads_are_automatically_enlisted() {
        let store = InMemoryEventStore::new();
        let metadata = metadata();
        let mut execution = CommandExecution::new(&store, &metadata);
        let mut first = execution.load::<TestAggregate>("first").await.unwrap();
        let mut second = execution.load::<TestAggregate>("second").await.unwrap();

        raise_on_both(&mut first, &mut second);
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

    trait WriteAction {
        fn write(&mut self);
    }

    impl WriteAction for AggregateInstance<TestAggregate> {
        fn write(&mut self) {
            self.raise(TestEvent);
        }
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
            writer.write();
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
        assert_eq!(
            receipt
                .correlation_id()
                .map(rostfrei_messaging_core::CorrelationId::as_str),
            Some("operation")
        );
        assert_eq!(receipt.causation_id(), None);
        for event in receipt.events() {
            assert_eq!(event.correlation_id(), receipt.correlation_id());
            assert_eq!(event.causation_id(), receipt.causation_id());
        }
    }

    #[tokio::test]
    async fn replay_rejects_changed_execution_metadata() {
        use rostfrei_messaging_core::{CausationId, CorrelationId};

        let executor = CommandExecutor::new(InMemoryEventStore::new());
        let handler = WriteHandler {
            calls: AtomicUsize::new(0),
        };
        let metadata = metadata().with_causation_id(CausationId::new("cause").unwrap());
        executor
            .execute(&handler, metadata.clone(), &())
            .await
            .unwrap();
        for changed in [
            CommandExecutionMetadata::new(
                metadata.operation_id().clone(),
                ContentFingerprint::digest("different-command"),
            )
            .with_bounded_context(metadata.bounded_context().unwrap().clone())
            .with_causation_id(metadata.causation_id().unwrap().clone()),
            metadata
                .clone()
                .with_correlation_id(CorrelationId::new("other-flow").unwrap()),
            metadata.with_causation_id(CausationId::new("other-cause").unwrap()),
        ] {
            let error = executor.execute(&handler, changed, &()).await.unwrap_err();
            assert!(matches!(
                error,
                CommandExecutionError::Store(error)
                    if error.kind() == EventStoreErrorKind::IdentityConflict
            ));
        }
        assert_eq!(handler.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn invalid_default_correlation_fails_before_handling_or_persistence() {
        for operation in ["operation with spaces", "opération"] {
            let store = InMemoryEventStore::new();
            let executor = CommandExecutor::new(store.clone());
            let handler = WriteHandler {
                calls: AtomicUsize::new(0),
            };
            let metadata = CommandExecutionMetadata::new(
                OperationId::new(operation).unwrap(),
                ContentFingerprint::digest("command"),
            )
            .with_bounded_context(metadata().bounded_context().unwrap().clone());

            let error = executor
                .execute(&handler, metadata.clone(), &())
                .await
                .unwrap_err();
            assert!(matches!(error, CommandExecutionError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest));
            let error = executor
                .simulate(&handler, metadata.clone(), &())
                .await
                .unwrap_err();
            assert!(matches!(error, SimulationError::Store(error)
                if error.kind() == EventStoreErrorKind::InvalidRequest));
            assert_eq!(handler.calls.load(Ordering::Relaxed), 0);
            assert!(
                store
                    .load_transaction_receipt_in_context(
                        metadata.bounded_context().unwrap(),
                        metadata.operation_id(),
                    )
                    .await
                    .unwrap()
                    .is_none()
            );

            let correlation = rostfrei_messaging_core::CorrelationId::new("explicit-flow").unwrap();
            let outcome = executor
                .execute(
                    &handler,
                    metadata.with_correlation_id(correlation.clone()),
                    &(),
                )
                .await
                .unwrap();
            let CommandOutcome::Accepted(CommandReceipt::Appended(events)) = outcome else {
                panic!("explicit correlation should allow execution");
            };
            assert_eq!(events[0].correlation_id(), Some(&correlation));
        }
    }

    struct MetadataObservingHandler;

    #[async_trait]
    impl CommandHandler<CommandExecutionMetadata> for MetadataObservingHandler {
        type Rejection = ();

        async fn handle(
            &self,
            expected: &CommandExecutionMetadata,
            execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<Self::Rejection> {
            assert_eq!(execution.metadata(), expected);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn execution_and_simulation_handlers_observe_normalized_metadata() {
        use rostfrei_messaging_core::{CausationId, CorrelationId};

        let metadata = metadata().with_causation_id(CausationId::new("cause").unwrap());
        for (input, expected) in [
            (
                metadata.clone(),
                metadata
                    .clone()
                    .with_correlation_id(CorrelationId::new("operation").unwrap()),
            ),
            (
                metadata
                    .clone()
                    .with_correlation_id(CorrelationId::new("earlier-operation").unwrap()),
                metadata.with_correlation_id(CorrelationId::new("earlier-operation").unwrap()),
            ),
        ] {
            let expected = expected.with_default_outcome_evidence::<CommandExecutionMetadata>();
            let store = InMemoryEventStore::new();
            let executor = CommandExecutor::new(store.clone());
            let outcome = executor
                .execute(&MetadataObservingHandler, input.clone(), &expected)
                .await
                .unwrap();
            assert_eq!(
                outcome,
                CommandOutcome::Accepted(CommandReceipt::AcceptedNoEvents)
            );
            let receipt = store
                .load_transaction_receipt_in_context(
                    expected.bounded_context().unwrap(),
                    expected.operation_id(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(receipt.correlation_id(), expected.correlation_id());
            assert_eq!(receipt.causation_id(), expected.causation_id());
            assert!(receipt.events().is_empty());
            assert_eq!(
                executor
                    .execute(&MetadataObservingHandler, input.clone(), &expected)
                    .await
                    .unwrap(),
                CommandOutcome::Accepted(CommandReceipt::ExactReplay(Vec::new()))
            );
            let simulation = executor
                .simulate(&MetadataObservingHandler, input, &expected)
                .await
                .unwrap();
            assert_eq!(simulation.decision(), &CommandDecision::Accepted);
        }
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
            raise_on_both(&mut first, &mut second);
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn two_loaded_aggregates_are_committed_atomically() {
        let store = InMemoryEventStore::new();
        let correlation = rostfrei_messaging_core::CorrelationId::new("earlier-operation").unwrap();
        let causation = rostfrei_messaging_core::CausationId::new("parent-event").unwrap();

        let outcome = CommandExecutor::new(store.clone())
            .execute(
                &TwoWriterHandler,
                metadata()
                    .with_correlation_id(correlation.clone())
                    .with_causation_id(causation.clone()),
                &(),
            )
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
        assert_eq!(receipt.correlation_id(), Some(&correlation));
        assert_eq!(receipt.causation_id(), Some(&causation));
        assert!(
            receipt
                .events()
                .iter()
                .all(|event| event.correlation_id() == Some(&correlation)
                    && event.causation_id() == Some(&causation))
        );
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
        failure_kind: EventStoreErrorKind,
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
    impl CommandCommitSession for ConflictOnceSession<'_> {
        async fn commit_outcome(
            self: Box<Self>,
            transaction: EventTransaction,
        ) -> Result<CompletedCommit, EventStoreError> {
            assert_eq!(self.loads.load(Ordering::Relaxed), 2);
            assert_eq!(self.session_id, self.store.attempts.load(Ordering::Relaxed));
            self.store.commit_outcome(transaction).await
        }
    }

    #[async_trait]
    impl OutcomeReader for ConflictOnceStore {
        async fn lookup_outcome(
            &self,
            key: &OutcomeKey,
        ) -> Result<OutcomeLookup<crate::EncodedOutcomeDetails>, EventStoreError> {
            self.inner.lookup_outcome(key).await
        }
    }

    impl ConflictOnceStore {
        async fn commit_outcome(
            &self,
            transaction: EventTransaction,
        ) -> Result<CompletedCommit, EventStoreError> {
            assert_eq!(
                transaction
                    .correlation_id()
                    .map(rostfrei_messaging_core::CorrelationId::as_str),
                Some(transaction.operation_id().as_str())
            );
            assert_eq!(transaction.causation_id(), None);
            if self.attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err(EventStoreError::new(
                    self.failure_kind,
                    "injected transaction conflict",
                ));
            }
            self.inner
                .begin_command()
                .await?
                .commit_outcome(transaction)
                .await
        }
    }

    #[async_trait]
    impl ExecutionStore for ConflictOnceStore {
        fn authority(&self) -> &crate::CommandAuthority {
            self.inner.authority()
        }
        async fn begin_command(
            &self,
        ) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
            Ok(Box::new(ConflictOnceSession {
                store: self,
                session_id: self.sessions.fetch_add(1, Ordering::Relaxed),
                loads: AtomicUsize::new(0),
            }))
        }
    }

    #[tokio::test]
    async fn a_conflict_retries_the_entire_handler_and_unit_of_work() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let sessions = Arc::new(AtomicUsize::new(0));
        let session_loads = Arc::new(AtomicUsize::new(0));
        let store: Arc<dyn ExecutionStore> = Arc::new(ConflictOnceStore {
            inner: InMemoryEventStore::new(),
            failure_kind: EventStoreErrorKind::Conflict,
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
    impl OutcomeReader for ReceiptAfterDecisionStore {
        async fn lookup_outcome(
            &self,
            key: &OutcomeKey,
        ) -> Result<OutcomeLookup<crate::EncodedOutcomeDetails>, EventStoreError> {
            if self.receipt_lookups.fetch_add(1, Ordering::Relaxed) == 0 {
                return Ok(OutcomeLookup::Missing);
            }
            self.inner.lookup_outcome(key).await
        }
    }

    #[async_trait]
    impl ExecutionStore for ReceiptAfterDecisionStore {
        fn authority(&self) -> &crate::CommandAuthority {
            self.inner.authority()
        }
        async fn begin_command(
            &self,
        ) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
            self.inner.begin_command().await
        }
    }

    struct RejectingHandler;

    struct TestRejectionCodec;
    impl crate::RejectionCodec<&'static str> for TestRejectionCodec {
        fn encode(&self, value: &&'static str) -> Result<crate::EncodedRejection, EventCodecError> {
            Ok(crate::EncodedRejection {
                codec_id: "test.static-rejection".to_owned(),
                codec_version: 1,
                bytes: value.as_bytes().to_vec(),
            })
        }
        fn decode(&self, value: &crate::EncodedRejection) -> Result<&'static str, EventCodecError> {
            if value.bytes == b"rejected" {
                Ok("rejected")
            } else {
                Err(EventCodecError::new(
                    crate::EventCodecErrorKind::MalformedPayload,
                    "unexpected test rejection",
                ))
            }
        }
    }

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
            .execute_with_rejection_codec(&RejectingHandler, metadata(), &(), &TestRejectionCodec)
            .await
            .unwrap();

        assert_eq!(outcome, CommandOutcome::Rejected("rejected"));
        assert!(store.load(&stream_id).await.unwrap().is_empty());
        assert!(
            store
                .load_completed_outcome_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .unwrap()
                .rejection()
                .is_some()
        );
        assert_eq!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::IdentityConflict
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

    async fn assert_rejected_tracking_before_concurrent_winner<Handler>(
        handler: &Handler,
        aggregate: &str,
        accepted: bool,
    ) where
        Handler: CommandHandler<(), Rejection = &'static str>,
    {
        let inner = InMemoryEventStore::new();
        let executor = CommandExecutor::new(inner.clone());
        if accepted {
            executor
                .execute(&EmptyHandler, metadata(), &())
                .await
                .unwrap();
        } else {
            executor
                .execute_with_rejection_codec(
                    &RejectingHandler,
                    metadata(),
                    &(),
                    &TestRejectionCodec,
                )
                .await
                .unwrap();
        }
        let before = inner
            .load_completed_outcome_in_context(
                metadata().bounded_context().unwrap(),
                metadata().operation_id(),
            )
            .await
            .unwrap()
            .unwrap();
        let store = Arc::new(ReceiptAfterDecisionStore {
            inner: inner.clone(),
            receipt_lookups: AtomicUsize::new(0),
        });
        let error = CommandExecutor::new(store.clone())
            .execute_with_rejection_codec(handler, metadata(), &(), &TestRejectionCodec)
            .await
            .expect_err("a concurrent winner must not hide invalid rejected tracking");
        assert!(matches!(error, CommandExecutionError::Store(error)
            if error.kind() == EventStoreErrorKind::InvalidRequest));
        assert_eq!(store.receipt_lookups.load(Ordering::Relaxed), 1);
        assert_eq!(
            inner
                .load_completed_outcome_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .unwrap(),
            before
        );
        assert!(
            inner
                .load(&stream_id_for::<TestAggregate>(aggregate).unwrap())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn accepted_winner_cannot_bypass_rejected_forgotten_tracking() {
        assert_rejected_tracking_before_concurrent_winner(
            &RejectedForgottenHandler,
            "rejected-forgotten",
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn rejected_winner_cannot_bypass_rejected_forgotten_tracking() {
        assert_rejected_tracking_before_concurrent_winner(
            &RejectedForgottenHandler,
            "rejected-forgotten",
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn accepted_winner_cannot_bypass_rejected_replaced_tracking() {
        assert_rejected_tracking_before_concurrent_winner(
            &RejectedReplacingHandler,
            "rejected-replaced",
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn rejected_winner_cannot_bypass_rejected_replaced_tracking() {
        assert_rejected_tracking_before_concurrent_winner(
            &RejectedReplacingHandler,
            "rejected-replaced",
            false,
        )
        .await;
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
            let stream_id = loaded.stream_id().clone();
            *loaded = AggregateInstance::new(stream_id);
            loaded.write();
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
            let _other = execution.load::<TestAggregate>("other-guard").await?;
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn automatically_enlisted_read_guards_without_writes_persist_acceptance() {
        let store = InMemoryEventStore::new();
        let outcome = CommandExecutor::new(store.clone())
            .execute(&NoEventsHandler, metadata(), &())
            .await
            .unwrap();

        assert_eq!(
            outcome,
            CommandOutcome::Accepted(CommandReceipt::AcceptedNoEvents)
        );
        assert!(
            store
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id()
                )
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn event_free_acceptance_retries_conflicts_but_never_accepts_write_failures() {
        for failure_kind in [
            EventStoreErrorKind::Conflict,
            EventStoreErrorKind::Unavailable,
        ] {
            let store = ConflictOnceStore {
                inner: InMemoryEventStore::new(),
                failure_kind,
                attempts: Arc::new(AtomicUsize::new(0)),
                sessions: Arc::new(AtomicUsize::new(0)),
                session_loads: Arc::new(AtomicUsize::new(0)),
            };
            let executor = CommandExecutor::new(store.clone());
            let result = executor.execute(&NoEventsHandler, metadata(), &()).await;
            let receipt = store
                .inner
                .load_transaction_receipt_in_context(
                    metadata().bounded_context().unwrap(),
                    metadata().operation_id(),
                )
                .await
                .unwrap();
            if failure_kind == EventStoreErrorKind::Conflict {
                assert_eq!(
                    result.unwrap(),
                    CommandOutcome::Accepted(CommandReceipt::AcceptedNoEvents)
                );
                assert_eq!(store.attempts.load(Ordering::Relaxed), 2);
                assert!(receipt.is_some());
            } else {
                assert!(
                    matches!(result, Err(CommandExecutionError::Store(error)) if error.kind() == EventStoreErrorKind::Unavailable)
                );
                assert_eq!(store.attempts.load(Ordering::Relaxed), 1);
                assert!(receipt.is_none());
            }
        }
    }

    struct EmptyHandler;

    #[async_trait]
    impl CommandHandler<()> for EmptyHandler {
        type Rejection = ();

        async fn handle(
            &self,
            _command: &(),
            _execution: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<()> {
            Ok(CommandDecision::Accepted)
        }
    }

    #[tokio::test]
    async fn acceptance_without_aggregates_is_context_scoped() {
        let store = InMemoryEventStore::new();
        for context in ["test-context", "other-context"] {
            let metadata = metadata().with_bounded_context(
                rostfrei_messaging_core::BoundedContextName::new(context).unwrap(),
            );
            let executor = CommandExecutor::new(store.clone());
            assert_eq!(
                executor
                    .execute(&EmptyHandler, metadata.clone(), &())
                    .await
                    .unwrap(),
                CommandOutcome::Accepted(CommandReceipt::AcceptedNoEvents)
            );
            let recreated = CommandExecutor::new(store.clone());
            assert_eq!(
                recreated
                    .execute(&EmptyHandler, metadata, &())
                    .await
                    .unwrap(),
                CommandOutcome::Accepted(CommandReceipt::ExactReplay(Vec::new()))
            );
        }
    }

    #[tokio::test]
    async fn eventful_contender_replays_a_concurrent_event_free_acceptance() {
        let inner = InMemoryEventStore::new();
        CommandExecutor::new(inner.clone())
            .execute(&EmptyHandler, metadata(), &())
            .await
            .unwrap();
        let store = ReceiptAfterDecisionStore {
            inner: inner.clone(),
            receipt_lookups: AtomicUsize::new(0),
        };
        let result = CommandExecutor::new(store)
            .with_max_conflict_retries(0)
            .execute(
                &WriteHandler {
                    calls: AtomicUsize::new(0),
                },
                metadata(),
                &(),
            )
            .await
            .unwrap();
        assert_eq!(
            result,
            CommandOutcome::Accepted(CommandReceipt::ExactReplay(Vec::new()))
        );
        assert!(
            inner
                .load(&stream_id_for::<TestAggregate>("writer").unwrap())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn no_event_decision_reconciles_a_receipt_committed_while_handling() {
        let inner = InMemoryEventStore::new();
        let metadata = metadata();
        let correlation_id =
            rostfrei_messaging_core::CorrelationId::new(metadata.operation_id().as_str()).unwrap();
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
        .unwrap()
        .with_correlation_id(correlation_id.clone());
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
                .with_bounded_context(metadata.bounded_context().unwrap().clone())
                .with_correlation_id(correlation_id),
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
