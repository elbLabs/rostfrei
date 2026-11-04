//! Read-only, bounded inspection of retained messages, independent of live capture.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{ObservationScope, TracePayloadPolicy};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogHistorical {
    pub application: String,
    pub scope: ObservationScope,
    pub messages_href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_href: Option<String>,
    pub aggregate_types: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub snapshot_aggregate_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate_href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_fixture_href: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalQuery {
    pub aggregate_type: Option<String>,
    pub aggregate_id: Option<String>,
    pub correlation_id: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub order: Option<HistoricalOrder>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoricalOrder {
    #[default]
    Source,
    Time,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalMessage {
    pub id: String,
    pub kind: &'static str,
    /// A response identifies its command, but does not recover its request or ancestry.
    pub evidence: &'static str,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate_id: Option<String>,
    /// Decimal strings preserve the complete u64 range in browser clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_version: Option<String>,
    pub stored_at: String,
    /// Exact broker timestamp; decimal strings avoid browser number rounding.
    pub stored_at_unix_nanos: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalPage {
    pub application: String,
    pub scope: ObservationScope,
    pub generation: String,
    pub order: HistoricalOrder,
    pub items: Vec<HistoricalMessage>,
    pub scanned_messages: usize,
    pub skipped_messages: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_href: Option<String>,
    /// Retention and independent per-stream horizons never prove a complete business flow.
    pub partial: bool,
}

#[derive(Clone, Copy, Debug, Error)]
pub enum HistoricalError {
    #[error("snapshot fixture export requires unredacted state and event payloads")]
    PayloadUnavailable,
    #[error("historical inspection is not configured for this scope")]
    NotConfigured,
    #[error("invalid historical query or cursor")]
    InvalidQuery,
    #[error("historical streams were reset; refresh to start a new read")]
    Reset,
    #[error("historical storage is unavailable")]
    Unavailable,
    #[error("historical read timed out")]
    Timeout,
    #[error("historical inspection capacity is exhausted")]
    Capacity,
}

#[async_trait]
pub trait HistoricalReader: Send + Sync {
    fn catalog(&self) -> CatalogHistorical;
    async fn read(
        &self,
        query: HistoricalQuery,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<HistoricalPage, HistoricalError>;
    async fn aggregate(
        &self,
        _query: HistoricalAggregateQuery,
        _policy: &dyn TracePayloadPolicy,
    ) -> Result<HistoricalAggregate, HistoricalError> {
        Err(HistoricalError::NotConfigured)
    }
    async fn snapshot_fixture(
        &self,
        _query: HistoricalAggregateQuery,
        _policy: &dyn TracePayloadPolicy,
    ) -> Result<crate::Fixture, HistoricalError> {
        Err(HistoricalError::NotConfigured)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalAggregateQuery {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub version: Option<String>,
    pub use_snapshots: Option<bool>,
    pub generation: Option<String>,
    pub snapshot_version: Option<String>,
    pub fixture_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalSnapshot {
    pub stream_version: String,
    pub replay_revision: String,
    pub schema_version: u32,
    pub state_bytes: usize,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalAggregate {
    pub application: String,
    pub scope: ObservationScope,
    pub generation: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub version: String,
    pub snapshot_version: String,
    pub snapshot_status: rostfrei_core::SnapshotStatus,
    pub events_read: usize,
    pub events_applied: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    pub snapshots: Vec<HistoricalSnapshot>,
}
