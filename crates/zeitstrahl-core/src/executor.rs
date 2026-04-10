use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::{
    Aggregate, AppendOutcome, CommandHandler, DecisionContext, EventBatch, EventCodec,
    EventCodecError, EventStore, EventStoreError, EventStoreErrorKind, ExecutionMetadata,
    ExpectedVersion, RecordedEvent, StreamId, StreamVersion,
};

const DEFAULT_MAX_CONFLICT_RETRIES: usize = 3;

pub struct Executor<S, C> {
    store: S,
    codec: C,
    maximum_conflict_retries: usize,
}

impl<S, C> Executor<S, C> {
    pub fn new(store: S, codec: C) -> Self {
        Self {
            store,
            codec,
            maximum_conflict_retries: DEFAULT_MAX_CONFLICT_RETRIES,
        }
    }

    #[must_use]
    pub fn with_max_conflict_retries(mut self, maximum_conflict_retries: usize) -> Self {
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
pub enum ExecutionOutcome {
    Appended(Vec<RecordedEvent>),
    ExactReplay(Vec<RecordedEvent>),
    NoEvents,
}

impl ExecutionOutcome {
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

#[derive(Debug, Error)]
pub enum ExecutionError<Rejection> {
    #[error("command rejected")]
    Rejected(Rejection),
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(transparent)]
    Codec(#[from] EventCodecError),
}

impl<S, C> Executor<S, C>
where
    S: EventStore,
{
    pub async fn execute<A, Command>(
        &self,
        metadata: ExecutionMetadata,
        command: &Command,
    ) -> Result<ExecutionOutcome, ExecutionError<<A as CommandHandler<Command>>::Rejection>>
    where
        A: Aggregate + CommandHandler<Command>,
        C: EventCodec<A>,
    {
        if metadata.stream_id().aggregate_type().as_str() != A::AGGREGATE_TYPE {
            return Err(invalid_request(format!(
                "aggregate type {} cannot execute stream type {}",
                A::AGGREGATE_TYPE,
                metadata.stream_id().aggregate_type()
            ))
            .into());
        }
        for attempt in 0..=self.maximum_conflict_retries {
            let history = self.store.load(metadata.stream_id()).await?;
            validate_history(metadata.stream_id(), &history)?;

            let mut aggregate = A::initial();
            for event in &history {
                let decoded = self.codec.decode(event)?;
                aggregate.apply(&decoded);
            }

            let prior_operation: Vec<_> = history
                .iter()
                .filter(|event| event.operation_id() == metadata.operation_id())
                .cloned()
                .collect();
            if !prior_operation.is_empty() {
                let exact = prior_operation.iter().all(|event| {
                    event.commit_id() == metadata.commit_id()
                        && event.operation_fingerprint() == metadata.operation_fingerprint()
                });
                if exact {
                    return Ok(ExecutionOutcome::ExactReplay(prior_operation));
                }
                return Err(EventStoreError::new(
                    EventStoreErrorKind::IdentityConflict,
                    "operation identity was reused with a different fingerprint or commit",
                )
                .into());
            }

            let mut pending = Vec::new();
            let mut context = DecisionContext::new(&mut aggregate, &mut pending);
            A::handle(command, &mut context).map_err(ExecutionError::Rejected)?;
            if pending.is_empty() {
                return Ok(ExecutionOutcome::NoEvents);
            }

            let mut encoded = Vec::with_capacity(pending.len());
            for (ordinal, event) in pending.iter().enumerate() {
                let ordinal = u32::try_from(ordinal).map_err(|_| {
                    invalid_request("command emitted more events than the supported ordinal range")
                })?;
                encoded.push(self.codec.encode(event, metadata.event_id(ordinal))?);
            }
            let batch = EventBatch::new(
                metadata.commit_id().clone(),
                metadata.operation_id().clone(),
                metadata.operation_fingerprint(),
                encoded,
            )
            .map_err(|error| invalid_request(error.to_string()))?;
            let expected_version = history.last().map_or(ExpectedVersion::NoStream, |event| {
                ExpectedVersion::Exact(event.stream_version())
            });

            match self
                .store
                .append(metadata.stream_id(), expected_version, batch)
                .await
            {
                Ok(AppendOutcome::Appended(events)) => {
                    return Ok(ExecutionOutcome::Appended(events));
                }
                Ok(AppendOutcome::ExactReplay(events)) => {
                    return Ok(ExecutionOutcome::ExactReplay(events));
                }
                Err(error)
                    if error.kind() == EventStoreErrorKind::Conflict
                        && attempt < self.maximum_conflict_retries => {}
                Err(error) => return Err(error.into()),
            }
        }

        unreachable!("the bounded retry loop always returns on its final attempt")
    }
}

fn validate_history(
    stream_id: &StreamId,
    history: &[RecordedEvent],
) -> Result<(), EventStoreError> {
    let mut event_ids = HashSet::with_capacity(history.len());
    let mut seen_commits = HashSet::new();
    let mut operations = HashMap::new();
    let mut current_commit = None;
    let mut ordinal = 0_u32;

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

        if current_commit.as_ref() != Some(event.commit_id()) {
            if !seen_commits.insert(event.commit_id()) {
                return Err(corrupt("events for one commit are not contiguous"));
            }
            current_commit = Some(event.commit_id().clone());
            ordinal = 0;
        }
        if event.commit_id() != &derive_commit_id(stream_id, event.operation_id()) {
            return Err(corrupt(
                "loaded history contains an invalid commit identity",
            ));
        }
        if event.event_id() != &derive_event_id(event.commit_id(), ordinal) {
            return Err(corrupt("loaded history contains an invalid event identity"));
        }
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| corrupt("a commit exceeds the supported event ordinal range"))?;

        let operation_identity = (event.commit_id(), event.operation_fingerprint());
        if let Some(previous) = operations.insert(event.operation_id(), operation_identity) {
            if previous != operation_identity {
                return Err(corrupt(
                    "loaded history reuses an operation identity across different commits",
                ));
            }
        }
    }
    Ok(())
}

fn corrupt(message: &'static str) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::CorruptHistory, message)
}

fn invalid_request(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}
