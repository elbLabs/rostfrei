//! Optional derived aggregate checkpoints. Original domain events remain authoritative.
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    marker::PhantomData,
    sync::Mutex,
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    Aggregate, ContentFingerprint, EventCodecError, EventCodecErrorKind, EventStoreError,
    HistoryCheckpoint, StreamId, StreamVersion,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SnapshotKey(String);
impl SnapshotKey {
    pub fn new(authority: &str, stream: &StreamId, replay_revision: &str, schema: u32) -> Self {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        for part in [
            authority,
            stream.aggregate_type().as_str(),
            stream.aggregate_id().as_str(),
            replay_revision,
            &schema.to_string(),
        ] {
            hash.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
            hash.update(part.as_bytes());
        }
        Self(ContentFingerprint::from_bytes(hash.finalize().into()).to_hex())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSnapshot {
    pub format: u32,
    pub replay_revision: String,
    pub schema_version: u32,
    pub checkpoint: HistoryCheckpoint,
    #[serde(with = "base64_bytes")]
    pub state: Vec<u8>,
    pub checksum: String,
}
impl StoredSnapshot {
    pub fn new(
        replay_revision: String,
        schema_version: u32,
        checkpoint: HistoryCheckpoint,
        state: Vec<u8>,
    ) -> Result<Self, EventStoreError> {
        let mut result = Self {
            format: 1,
            replay_revision,
            schema_version,
            checkpoint,
            state,
            checksum: String::new(),
        };
        result.checksum = result.digest()?;
        Ok(result)
    }
    pub const fn version(&self) -> StreamVersion {
        StreamVersion::new(self.checkpoint.version)
    }
    pub fn validate(&self) -> Result<(), EventStoreError> {
        if self.format != 1
            || self.schema_version == 0
            || self.replay_revision.is_empty()
            || self.replay_revision.len() > 128
            || self.checkpoint.version == 0
            || self.checksum != self.digest()?
        {
            return Err(crate::replay::invalid(
                "invalid snapshot format, revision, version, or checksum",
            ));
        }
        Ok(())
    }
    /// Replay anchors may contain differently sized derived indexes. Publication
    /// converges when the same logical boundary and replay semantics produce identical state.
    pub fn same_state(&self, other: &Self) -> bool {
        self.format == other.format
            && self.replay_revision == other.replay_revision
            && self.schema_version == other.schema_version
            && self.state == other.state
            && self.checkpoint.authority == other.checkpoint.authority
            && self.checkpoint.aggregate_type == other.checkpoint.aggregate_type
            && self.checkpoint.aggregate_id == other.checkpoint.aggregate_id
            && self.checkpoint.version == other.checkpoint.version
    }
    fn digest(&self) -> Result<String, EventStoreError> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&(
            self.format,
            &self.replay_revision,
            self.schema_version,
            &self.checkpoint,
        ))
        .map_err(|error| crate::replay::invalid(error.to_string()))?;
        let mut hash = Sha256::new();
        hash.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
        hash.update(bytes);
        hash.update(&self.state);
        Ok(ContentFingerprint::from_bytes(hash.finalize().into()).to_hex())
    }
}

#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn load(
        &self,
        key: &SnapshotKey,
        at_or_before: StreamVersion,
    ) -> Result<Option<StoredSnapshot>, EventStoreError>;
    /// Idempotent publication; different content at the same key/version is a conflict.
    async fn save(
        &self,
        key: &SnapshotKey,
        snapshot: &StoredSnapshot,
    ) -> Result<(), EventStoreError>;

    /// Bounded, newest-first inventory for one current authority/replay identity.
    async fn list(&self, key: &SnapshotKey) -> Result<Vec<StoredSnapshot>, EventStoreError> {
        let mut snapshots = Vec::new();
        let mut before = StreamVersion::new(u64::MAX);
        while snapshots.len() < 64 {
            let Some(snapshot) = self.load(key, before).await? else {
                break;
            };
            let version = snapshot.version().value();
            if version == 0 || version > before.value() {
                return Err(crate::replay::invalid("snapshot inventory did not advance"));
            }
            before = StreamVersion::new(version.saturating_sub(1));
            snapshots.push(snapshot);
        }
        Ok(snapshots)
    }
}

/// Portable state for a fixture. Native authority and replay anchors are rebuilt
/// only after validating this state against the fixture's complete original events.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotState {
    pub replay_revision: String,
    pub schema_version: u32,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}
impl From<&StoredSnapshot> for SnapshotState {
    fn from(value: &StoredSnapshot) -> Self {
        Self {
            replay_revision: value.replay_revision.clone(),
            schema_version: value.schema_version,
            data: value.state.clone(),
        }
    }
}

pub struct AggregateReconstruction {
    pub version: StreamVersion,
    pub state: Vec<u8>,
    pub report: RebuildReport,
}

#[derive(Default)]
pub struct InMemorySnapshotStore(Mutex<HashMap<SnapshotKey, BTreeMap<u64, StoredSnapshot>>>);
#[async_trait]
impl SnapshotStore for InMemorySnapshotStore {
    async fn load(
        &self,
        key: &SnapshotKey,
        at_or_before: StreamVersion,
    ) -> Result<Option<StoredSnapshot>, EventStoreError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .and_then(|entries| {
                entries
                    .range(..=at_or_before.value())
                    .next_back()
                    .map(|(_, value)| value.clone())
            }))
    }
    async fn save(
        &self,
        key: &SnapshotKey,
        snapshot: &StoredSnapshot,
    ) -> Result<(), EventStoreError> {
        snapshot.validate()?;
        let mut entries = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let versions = entries.entry(key.clone()).or_default();
        if let Some(old) = versions.get(&snapshot.checkpoint.version) {
            if old.same_state(snapshot) {
                return Ok(());
            }
            return Err(EventStoreError::new(
                crate::EventStoreErrorKind::Conflict,
                "snapshot target already has different state",
            ));
        }
        versions.insert(snapshot.checkpoint.version, snapshot.clone());
        while versions.len() > 64 {
            versions.pop_first();
        }
        drop(entries);
        Ok(())
    }
}

pub trait SnapshotCodec<A: Aggregate>: Send + Sync {
    fn encode(&self, state: &A::State) -> Result<Vec<u8>, EventCodecError>;
    fn decode(&self, state: &[u8]) -> Result<A::State, EventCodecError>;
}
pub struct JsonSnapshotCodec<A>(PhantomData<fn() -> A>);
impl<A> Default for JsonSnapshotCodec<A> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<A: Aggregate> SnapshotCodec<A> for JsonSnapshotCodec<A>
where
    A::State: Serialize + DeserializeOwned,
{
    fn encode(&self, state: &A::State) -> Result<Vec<u8>, EventCodecError> {
        // Value's ordered object representation makes HashMap serialization deterministic.
        serde_json::to_value(state)
            .and_then(|mut value| {
                value.sort_all_objects();
                serde_json::to_vec(&value)
            })
            .map_err(|error| {
                EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
            })
    }
    fn decode(&self, bytes: &[u8]) -> Result<A::State, EventCodecError> {
        serde_json::from_slice(bytes).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotPolicy {
    Manual,
    EveryEvents(std::num::NonZeroU64),
    RebuildBudget {
        minimum_events: std::num::NonZeroU64,
        budget: Duration,
    },
}
impl SnapshotPolicy {
    pub(crate) fn recommends(&self, distance: u64, elapsed: Duration) -> bool {
        match self {
            Self::Manual => false,
            Self::EveryEvents(count) => distance >= count.get(),
            Self::RebuildBudget {
                minimum_events,
                budget,
            } => distance >= minimum_events.get() && elapsed >= *budget,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotStatus {
    Disabled,
    Miss,
    Hit,
    InvalidState,
    InvalidCheckpoint,
    InvalidAnchor,
    Unavailable,
    Timeout,
}

#[derive(Clone, Debug, Serialize)]
pub struct RebuildReport {
    pub authority: Option<String>,
    pub replay_revision: Option<String>,
    pub target_version: Option<u64>,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub operation_id: Option<String>,
    pub snapshot_version: u64,
    pub head_version: u64,
    pub events_applied: usize,
    pub events_read: usize,
    pub event_bytes: usize,
    pub total_ns: u64,
    pub snapshot_lookup_ns: u64,
    /// Included in `snapshot_lookup_ns`, not an additional disjoint phase.
    pub snapshot_decode_ns: u64,
    pub history_ns: u64,
    pub validation_ns: u64,
    pub decode_ns: u64,
    pub apply_ns: u64,
    pub snapshot_status: SnapshotStatus,
    pub error: Option<String>,
    pub snapshot_recommended: bool,
}

/// Bounded process-local diagnostics; identities belong here, not in metric labels.
pub struct RebuildDiagnostics {
    capacity: usize,
    reports: Mutex<VecDeque<RebuildReport>>,
    observed: std::sync::atomic::AtomicU64,
}
impl RebuildDiagnostics {
    pub const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            reports: Mutex::new(VecDeque::new()),
            observed: std::sync::atomic::AtomicU64::new(0),
        }
    }
    pub fn recent(&self) -> Vec<RebuildReport> {
        self.reports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
    /// Total load observations, including reports evicted from the bounded window.
    pub fn observed(&self) -> u64 {
        self.observed.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Slowest loads in the retained window; this is not a census of all aggregates.
    pub fn slowest(&self, limit: usize) -> Vec<RebuildReport> {
        let mut reports = self.recent();
        reports.sort_by_key(|report| std::cmp::Reverse(report.total_ns));
        reports.truncate(limit);
        reports
    }
    pub(crate) fn record(&self, report: RebuildReport) {
        self.observed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.capacity == 0 {
            return;
        }
        let mut reports = self
            .reports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while reports.len() >= self.capacity.min(4096) {
            reports.pop_front();
        }
        reports.push_back(report);
    }
}

#[derive(Clone, Debug)]
pub struct SnapshotRequest {
    pub authority: String,
    pub stream: StreamId,
    pub version: StreamVersion,
}

pub fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

pub mod base64_bytes {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        STANDARD
            .decode(String::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}
