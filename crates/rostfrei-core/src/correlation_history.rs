//! Read-only correlation evidence from retained event sources.
//!
//! Correlation groups messages; only explicit causation identifies a direct
//! cause. A read neither proves flow completion nor recovers expired messages.

use async_trait::async_trait;
use rostfrei_messaging_core::{CorrelationId, MessageTimestamp};
use serde::{Deserialize, Serialize};

use crate::{EventStoreError, RecordedEvent};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CorrelationRef {
    Domain(DomainEventRef),
    Integration(IntegrationEventRef),
}

impl CorrelationRef {
    pub const fn occurred_at(&self) -> MessageTimestamp {
        match self {
            Self::Domain(reference) => reference.occurred_at,
            Self::Integration(reference) => reference.occurred_at,
        }
    }

    pub fn context(&self) -> &str {
        match self {
            Self::Domain(reference) => &reference.context,
            Self::Integration(reference) => &reference.context,
        }
    }

    pub fn identity(&self) -> (&'static str, &str, &str) {
        match self {
            Self::Domain(reference) => ("domain", &reference.context, &reference.event_id),
            Self::Integration(reference) => {
                ("integration", &reference.context, &reference.message_id)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DomainEventRef {
    pub context: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub stream_version: u64,
    pub event_id: String,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub occurred_at: MessageTimestamp,
}

impl DomainEventRef {
    pub fn from_recorded(
        context: impl Into<String>,
        event: &RecordedEvent,
        occurred_at: MessageTimestamp,
    ) -> Self {
        Self {
            context: context.into(),
            aggregate_type: event.stream_id().aggregate_type().as_str().to_owned(),
            aggregate_id: event.stream_id().aggregate_id().as_str().to_owned(),
            stream_version: event.stream_version().value(),
            event_id: event.event_id().as_str().to_owned(),
            event_type: event.event_type().to_owned(),
            causation_id: event.causation_id().map(|id| id.as_str().to_owned()),
            occurred_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IntegrationEventRef {
    pub context: String,
    pub subject: String,
    pub message_id: String,
    pub event_type: String,
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub occurred_at: MessageTimestamp,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CorrelationChain {
    pub correlation_id: String,
    pub refs: Vec<CorrelationRef>,
    /// The read reached a resource bound; additional retained matches may exist.
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationSourceKind {
    Domain,
    Integration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorrelationSourceSnapshot {
    /// Absent for an application-wide integration source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub kind: CorrelationSourceKind,
    pub stream: String,
    /// Inclusive source sequence cutoff, encoded without JavaScript precision loss.
    pub cutoff_sequence: String,
}

/// Independently captured source cutoffs, not a cross-source atomic snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorrelationSnapshot {
    pub sources: Vec<CorrelationSourceSnapshot>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CorrelationRead {
    pub chains: Vec<CorrelationChain>,
    pub snapshot: CorrelationSnapshot,
}

#[async_trait]
pub trait CorrelationHistory: Send + Sync {
    /// Reads retained matches at one cutoff per configured source. A source
    /// failure is an error, never a successful empty or complete result.
    async fn load_correlations(
        &self,
        ids: &[CorrelationId],
    ) -> Result<CorrelationRead, EventStoreError>;
}
