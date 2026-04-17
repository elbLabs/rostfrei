use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::{
    Aggregate, AggregateInstance, AppendOutcome, CommandHandler, EventBatch, EventCodec,
    EventCodecError, EventHistory, EventStore, EventStoreError, EventStoreErrorKind,
    ExecutionMetadata, ExpectedVersion, JsonEventCodec, NewEvent, RecordedEvent, StreamId,
    StreamVersion,
};

const DEFAULT_MAX_CONFLICT_RETRIES: usize = 3;

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
pub struct SimulationOutcome<Rejection> {
    base_version: StreamVersion,
    decision: SimulationDecision<Rejection>,
}

impl<Rejection> SimulationOutcome<Rejection> {
    pub const fn base_version(&self) -> StreamVersion {
        self.base_version
    }

    pub const fn decision(&self) -> &SimulationDecision<Rejection> {
        &self.decision
    }

    pub fn into_parts(self) -> (StreamVersion, SimulationDecision<Rejection>) {
        (self.base_version, self.decision)
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

impl<S, C> Executor<S, C>
where
    S: EventHistory,
{
    pub async fn simulate<A, Command>(
        &self,
        metadata: ExecutionMetadata,
        command: &Command,
    ) -> Result<SimulationOutcome<<A as CommandHandler<Command>>::Rejection>, SimulationError>
    where
        A: Aggregate + CommandHandler<Command>,
        C: EventCodec<A>,
    {
        let (aggregate, history) = self.load_and_replay::<A>(metadata.stream_id()).await?;
        let base_version = current_version(&history);
        let decision = match self.decide::<A, Command>(&metadata, command, aggregate)? {
            SimulationDecision::Accepted(events) => {
                let events = prepare_batch(&metadata, events)?
                    .map_or_else(Vec::new, EventBatch::into_events);
                SimulationDecision::Accepted(events)
            }
            SimulationDecision::Rejected(rejection) => SimulationDecision::Rejected(rejection),
        };
        Ok(SimulationOutcome {
            base_version,
            decision,
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

    fn decide<A, Command>(
        &self,
        metadata: &ExecutionMetadata,
        command: &Command,
        mut aggregate: AggregateInstance<A>,
    ) -> Result<SimulationDecision<<A as CommandHandler<Command>>::Rejection>, SimulationError>
    where
        A: Aggregate + CommandHandler<Command>,
        C: EventCodec<A>,
    {
        if let Err(rejection) = A::handle(command, &mut aggregate) {
            return Ok(SimulationDecision::Rejected(rejection));
        }

        let mut encoded = Vec::with_capacity(aggregate.uncommitted_events().len());
        for (ordinal, event) in aggregate.uncommitted_events().iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                invalid_request("command emitted more events than the supported ordinal range")
            })?;
            encoded.push(self.codec.encode(event, metadata.event_id(ordinal))?);
        }
        Ok(SimulationDecision::Accepted(encoded))
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
        A: Aggregate + CommandHandler<Command>,
        C: EventCodec<A>,
    {
        let mut remaining_conflict_retries = self.maximum_conflict_retries;
        loop {
            let (aggregate, history) = self.load_and_replay::<A>(metadata.stream_id()).await?;

            let prior_operation: Vec<_> = history
                .iter()
                .filter(|event| event.operation_id() == metadata.operation_id())
                .cloned()
                .collect();
            if !prior_operation.is_empty() {
                let exact = prior_operation.iter().all(|event| {
                    event.commit_id() == metadata.commit_id()
                        && event.operation_fingerprint() == metadata.operation_fingerprint()
                        && event.correlation_id() == metadata.correlation_id()
                        && event.causation_id() == metadata.causation_id()
                });
                if exact {
                    return Ok(CommandOutcome::Accepted(CommandReceipt::ExactReplay(
                        prior_operation,
                    )));
                }
                return Err(EventStoreError::new(
                    EventStoreErrorKind::IdentityConflict,
                    "operation identity was reused with a different fingerprint or commit",
                )
                .into());
            }

            let events = match self.decide::<A, Command>(&metadata, command, aggregate)? {
                SimulationDecision::Accepted(events) => events,
                SimulationDecision::Rejected(rejection) => {
                    return Ok(CommandOutcome::Rejected(rejection));
                }
            };
            let Some(batch) = prepare_batch(&metadata, events)? else {
                return Ok(CommandOutcome::Accepted(CommandReceipt::NoEvents));
            };
            let expected_version = match current_version(&history) {
                StreamVersion::ZERO => ExpectedVersion::NoStream,
                version => ExpectedVersion::Exact(version),
            };

            match self
                .store
                .append(metadata.stream_id(), expected_version, batch)
                .await
            {
                Ok(AppendOutcome::Appended(events)) => {
                    return Ok(CommandOutcome::Accepted(CommandReceipt::Appended(events)));
                }
                Ok(AppendOutcome::ExactReplay(events)) => {
                    return Ok(CommandOutcome::Accepted(CommandReceipt::ExactReplay(
                        events,
                    )));
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

fn prepare_batch(
    metadata: &ExecutionMetadata,
    events: Vec<NewEvent>,
) -> Result<Option<EventBatch>, EventStoreError> {
    if events.is_empty() {
        return Ok(None);
    }
    let mut batch = EventBatch::new(
        metadata.commit_id().clone(),
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

    const CONST_EXECUTOR: Executor<(), ()> =
        Executor::with_codec((), ()).with_max_conflict_retries(4);
    const DEFAULT_CONST_EXECUTOR: Executor<()> = Executor::new(());

    #[test]
    fn executor_configuration_is_const_constructible() {
        assert_eq!(*CONST_EXECUTOR.store(), ());
        assert_eq!(*CONST_EXECUTOR.codec(), ());
        assert_eq!(*DEFAULT_CONST_EXECUTOR.store(), ());
    }
}
