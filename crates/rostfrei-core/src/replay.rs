//! Checked replay boundaries. Logical versions are independent of adapter positions.
use serde::{Deserialize, Serialize};

use crate::{
    ContentFingerprint, EventStoreError, EventStoreErrorKind, RecordedEvent, StreamId,
    StreamVersion,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCheckpoint {
    pub authority: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub version: u64,
    /// Adapter-owned evidence; never interpreted by domain code.
    #[serde(with = "crate::snapshots::base64_bytes")]
    pub position: Vec<u8>,
}

impl HistoryCheckpoint {
    pub fn validate_for(&self, authority: &str, stream: &StreamId) -> Result<(), EventStoreError> {
        if self.authority != authority
            || self.aggregate_type != stream.aggregate_type().as_str()
            || self.aggregate_id != stream.aggregate_id().as_str()
            || self.version == 0
        {
            return Err(invalid(
                "checkpoint belongs to another history or has version zero",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ReplayHistory {
    pub base_version: StreamVersion,
    pub events: Vec<RecordedEvent>,
    pub checkpoint: Option<HistoryCheckpoint>,
    /// Logical events examined by the successful replay source, including prefixes
    /// fetched by compatibility/audited readers. Excludes abandoned partial reads,
    /// consumer prefetch, and control lookups; transport counters measure those costs.
    pub events_read: usize,
}

impl ReplayHistory {
    pub fn version(&self) -> StreamVersion {
        self.events
            .last()
            .map_or(self.base_version, RecordedEvent::stream_version)
    }
}

pub fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}

/// Default compatibility reader. Adapters can override this to avoid fetching the prefix.
pub fn replay_from_history(
    authority: Option<&str>,
    stream: &StreamId,
    events: &[RecordedEvent],
    after: Option<&HistoryCheckpoint>,
    through: Option<StreamVersion>,
) -> Result<ReplayHistory, EventStoreError> {
    crate::executor::validate_history_from(stream, events, StreamVersion::ZERO)?;
    let head = events
        .last()
        .map_or(StreamVersion::ZERO, RecordedEvent::stream_version);
    let target = through.unwrap_or(head);
    if target > head {
        return Err(invalid(
            "snapshot/replay target is beyond the committed head",
        ));
    }
    let end = usize::try_from(target.value())
        .map_err(|_| invalid("version exceeds addressable history"))?;
    let selected = events
        .get(..end)
        .ok_or_else(|| invalid("target is outside history"))?;
    if let Some(last) = selected.last()
        && last.commit_event_ordinal().checked_add(1) != Some(last.commit_event_count())
    {
        return Err(invalid(
            "snapshot/replay target must be the last event of a complete commit",
        ));
    }
    let base = after.map_or(0, |checkpoint| checkpoint.version);
    if base > target.value() {
        return Err(invalid("checkpoint is newer than replay target"));
    }
    if let Some(checkpoint) = after {
        checkpoint.validate_for(
            authority.ok_or_else(|| invalid("history has no checkpoint authority"))?,
            stream,
        )?;
        let prefix = usize::try_from(base)
            .ok()
            .and_then(|index| selected.get(..index))
            .ok_or_else(|| invalid("checkpoint is outside history"))?;
        if prefix.last().is_none_or(|last| {
            last.commit_event_ordinal().checked_add(1) != Some(last.commit_event_count())
        }) {
            return Err(invalid("checkpoint must end at a complete commit"));
        }
        if checkpoint.position != history_digest(prefix)?.as_bytes() {
            return Err(EventStoreError::new(
                EventStoreErrorKind::CorruptHistory,
                "checkpoint prefix does not match history",
            ));
        }
    }
    let checkpoint = authority
        .filter(|_| target != StreamVersion::ZERO)
        .map(|authority| {
            Ok::<_, EventStoreError>(HistoryCheckpoint {
                authority: authority.to_owned(),
                aggregate_type: stream.aggregate_type().as_str().to_owned(),
                aggregate_id: stream.aggregate_id().as_str().to_owned(),
                version: target.value(),
                position: history_digest(selected)?.as_bytes().to_vec(),
            })
        })
        .transpose()?;
    let start = usize::try_from(base)
        .map_err(|_| invalid("checkpoint version exceeds addressable history"))?;
    Ok(ReplayHistory {
        base_version: StreamVersion::new(base),
        events: selected.get(start..).unwrap_or_default().to_vec(),
        checkpoint,
        events_read: events.len(),
    })
}

fn history_digest(events: &[RecordedEvent]) -> Result<ContentFingerprint, EventStoreError> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for event in events {
        let bytes = serde_json::to_vec(&(
            event.event_id().as_str(),
            event.commit_id().as_str(),
            event.operation_id().as_str(),
            event.operation_fingerprint().to_hex(),
            event.stream_version().value(),
            event.commit_event_ordinal(),
            event.commit_event_count(),
            event.event_type(),
            event.schema_version(),
            event.payload(),
            event.actor(),
            event
                .correlation_id()
                .map(rostfrei_messaging_core::CorrelationId::as_str),
            event
                .causation_id()
                .map(rostfrei_messaging_core::CausationId::as_str),
        ))
        .map_err(|error| invalid(error.to_string()))?;
        hash.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
        hash.update(bytes);
    }
    Ok(ContentFingerprint::from_bytes(hash.finalize().into()))
}
