//! Framework-owned index from a correlation to the committed messages carrying it.
//! Entries hold message metadata only; payloads stay in their authoritative stores.

use std::collections::BTreeMap;

use async_trait::async_trait;
use rostfrei_messaging_core::{CorrelationId, MessageTimestamp};
use serde::{Deserialize, Serialize};

use crate::{ReadModelError, RecordedEvent};

pub const CORRELATION_INDEX_SCHEMA_VERSION: u32 = 1;
/// Stored references per correlation. Further references set `truncated`.
pub const MAX_CORRELATION_REFS: usize = 2048;
/// References per KV value. A worst-case reference encodes to about 2 KiB, so a
/// full page stays well within the default 256 KiB value limit.
pub const CORRELATION_REFS_PER_PAGE: usize = 64;
pub const MAX_CORRELATION_PAGES: usize = MAX_CORRELATION_REFS / CORRELATION_REFS_PER_PAGE;

/// One page of a correlation. Page 0 is addressed by the correlation; later
/// pages append `/<page>`. Readers combine pages until one is not full.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorrelationIndexEntry {
    pub refs: Vec<CorrelationRef>,
    pub truncated: bool,
}

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

    /// Duplicate deliveries of one message resolve to the same identity.
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
#[serde(deny_unknown_fields)]
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

/// Integration streams have bounded retention. The reference keeps enough
/// metadata to remain meaningful after the message itself has expired.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationEventRef {
    pub context: String,
    pub subject: String,
    pub message_id: String,
    pub event_type: String,
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub occurred_at: MessageTimestamp,
    /// Position in the integration-event stream, used to check retention.
    pub sequence: u64,
}

/// A correlation assembled from all of its pages, ordered by `occurred_at`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CorrelationChain {
    pub refs: Vec<CorrelationRef>,
    pub truncated: bool,
}

/// Events not yet indexed, per bounded context. Zero everywhere means the index
/// was complete for every registered source when the lag was computed.
pub type CorrelationIndexLag = BTreeMap<String, u64>;

/// Read side of the correlation index. Freshness is computed by the
/// implementation against its sources, never by comparing positions in clients.
#[async_trait]
pub trait CorrelationIndexReader: Send + Sync {
    async fn chain(
        &self,
        correlation_id: &CorrelationId,
    ) -> Result<CorrelationChain, ReadModelError>;

    async fn lag(&self) -> Result<CorrelationIndexLag, ReadModelError>;

    /// Whether the referenced message is still retained by its stream.
    async fn integration_available(
        &self,
        reference: &IntegrationEventRef,
    ) -> Result<bool, ReadModelError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(id: &str, at: u64) -> CorrelationRef {
        CorrelationRef::Domain(DomainEventRef {
            context: "ordering".to_owned(),
            aggregate_type: "ordering/order".to_owned(),
            aggregate_id: "O1".to_owned(),
            stream_version: 1,
            event_id: id.to_owned(),
            event_type: "order-placed".to_owned(),
            causation_id: None,
            occurred_at: MessageTimestamp::from_unix_milliseconds(at).unwrap(),
        })
    }

    #[test]
    fn references_are_tagged_by_kind_and_identified_per_context() {
        let reference = domain("event:1", 7);
        let encoded = serde_json::to_value(&reference).unwrap();
        assert_eq!(encoded["kind"], "domain");
        assert_eq!(encoded["occurred_at"], 7);
        assert!(encoded.get("causation_id").is_none());
        assert_eq!(
            serde_json::from_value::<CorrelationRef>(encoded).unwrap(),
            reference
        );
        assert_eq!(reference.identity(), ("domain", "ordering", "event:1"));
    }

    #[test]
    fn page_capacity_divides_the_reference_cap() {
        assert_eq!(
            MAX_CORRELATION_PAGES.checked_mul(CORRELATION_REFS_PER_PAGE),
            Some(MAX_CORRELATION_REFS)
        );
    }
}
