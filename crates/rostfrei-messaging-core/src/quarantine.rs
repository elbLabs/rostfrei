//! Read-only inspection of retained failed deliveries, independent of command execution.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ApplicationName, CallerMetadata, CorrelationId, TraceContext, TrafficScope};

pub const DEFAULT_QUARANTINE_PAGE_SIZE: usize = 25;
pub const MAX_QUARANTINE_PAGE_SIZE: usize = 100;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuarantineMessageKind {
    Command,
    CommandResponse,
    IntegrationEvent,
}

impl QuarantineMessageKind {
    pub const fn subject_segment(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::CommandResponse => "command-response",
            Self::IntegrationEvent => "integration",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuarantineFailureKind {
    InvalidSourceMessage,
    DeliveryAttemptsExhausted,
    HandlerFailure,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct QuarantineFilter {
    pub kind: Option<QuarantineMessageKind>,
    pub context: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantineQuery {
    pub filter: QuarantineFilter,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for QuarantineQuery {
    fn default() -> Self {
        Self {
            filter: QuarantineFilter::default(),
            limit: DEFAULT_QUARANTINE_PAGE_SIZE,
            cursor: None,
        }
    }
}

/// These are inspection diagnostics, not inferred application failure causes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuarantineDiagnostic {
    InvalidRecord,
    RecordTooLarge,
    InvalidSourceAddress,
    InvalidPayloadEncoding,
    PayloadTruncated,
}

/// Captured evidence. Protocol adapters must apply their payload policy before serialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantinedMessage {
    pub message_id: String,
    pub address: String,
    pub payload_base64: String,
    pub payload_size: Option<usize>,
    pub payload_sha256: Option<String>,
    pub payload_truncated: bool,
    pub metadata: CallerMetadata,
    pub trace_context: Option<TraceContext>,
    /// Transport identity; absent on legacy records or invalid source headers.
    pub correlation_id: Option<CorrelationId>,
    pub reason: String,
    pub failure_kind: Option<QuarantineFailureKind>,
    pub attempt: u32,
    pub pending: u64,
    pub source_sequence: u64,
    pub consumer_sequence: u64,
    pub source_stream: String,
    pub source_consumer: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantineEntry {
    /// Opaque identity includes storage generation, not just a sequence number.
    pub id: String,
    pub stored_at: String,
    pub subject: String,
    pub message: Option<QuarantinedMessage>,
    pub diagnostics: Vec<QuarantineDiagnostic>,
    /// Bounded evidence for a malformed quarantine record, subject to the same payload policy.
    pub invalid_record_base64: Option<String>,
    pub invalid_record_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantinePage {
    pub items: Vec<QuarantineEntry>,
    pub next_cursor: Option<String>,
    /// Retained records across all subjects at the start of this read, not unresolved incidents.
    pub retained_messages: u64,
    /// Stable upper bound for this traversal. Retention can still remove older records.
    pub snapshot_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum QuarantineReadError {
    #[error("invalid quarantine query")]
    InvalidQuery,
    #[error("invalid quarantine record identity")]
    InvalidId,
    #[error("invalid quarantine cursor or changed filters")]
    InvalidCursor,
    #[error("quarantine stream was reset or recreated; restart inspection")]
    StaleGeneration,
    #[error("quarantine record was not found or has expired")]
    NotFound,
    #[error("quarantine reader is unavailable")]
    Unavailable,
    #[error("quarantine read timed out")]
    Timeout,
    #[error("quarantine stream does not match the configured application and traffic scope")]
    InvalidTopology,
}

#[async_trait]
pub trait QuarantineReader: Send + Sync {
    fn application(&self) -> &ApplicationName;

    fn traffic_scope(&self) -> TrafficScope;

    /// Lists in ascending storage sequence within a finite, generation-bound snapshot.
    async fn list(&self, query: QuarantineQuery) -> Result<QuarantinePage, QuarantineReadError>;

    async fn get(&self, id: &str) -> Result<QuarantineEntry, QuarantineReadError>;
}
