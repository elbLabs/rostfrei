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

/// Events not yet indexed, per bounded context.
///
/// `None` means at least one source of the context has no running projector,
/// so its count cannot shrink on its own. Zero everywhere means the index was
/// complete for every registered source when the lag was computed.
pub type CorrelationIndexLag = BTreeMap<String, Option<u64>>;

/// Stream a correlation index source projects from.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationSourceKind {
    /// The bounded context's event store.
    Domain,
    /// The integration events the bounded context publishes.
    Integration,
}

impl CorrelationSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Integration => "integration",
        }
    }
}

/// Whether a projector is currently consuming a source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationSourceState {
    Active,
    /// No projector consumes the source; unindexed events stay unindexed.
    Inactive,
}

/// Progress of one registered source of the correlation index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorrelationSourceFreshness {
    pub context: String,
    pub kind: CorrelationSourceKind,
    pub stream: String,
    pub state: CorrelationSourceState,
    /// Matching source messages after the source's checkpoint.
    pub behind: u64,
    /// Stream sequences after a checkpoint that the stream no longer retained
    /// when they were due for indexing, for example messages aged out while no
    /// projector ran. Their references are missing; not every skipped sequence
    /// needs to match the source. Cumulative until the index is rebuilt.
    pub skipped: u64,
}

/// Freshness of the correlation index as of one read: per-context `lag` and the
/// state of every registered source.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorrelationIndexFreshness {
    pub lag: CorrelationIndexLag,
    pub sources: Vec<CorrelationSourceFreshness>,
}

impl CorrelationIndexFreshness {
    /// Orders sources by context and kind and derives `lag` from them. A
    /// context's lag is the sum of its sources, or `None` if any is inactive.
    pub fn from_sources(mut sources: Vec<CorrelationSourceFreshness>) -> Self {
        sources.sort_by(|left, right| {
            (&left.context, left.kind, &left.stream).cmp(&(
                &right.context,
                right.kind,
                &right.stream,
            ))
        });
        let mut lag = CorrelationIndexLag::new();
        for source in &sources {
            let total = lag.entry(source.context.clone()).or_insert(Some(0));
            *total = match (source.state, *total) {
                (CorrelationSourceState::Active, Some(total)) => {
                    Some(total.saturating_add(source.behind))
                }
                _ => None,
            };
        }
        Self { lag, sources }
    }

    /// At least one source is registered, every source has a running projector
    /// and none is behind. Only then does a missing correlation prove absence.
    pub fn is_complete(&self) -> bool {
        !self.sources.is_empty()
            && self
                .sources
                .iter()
                .all(|source| source.state == CorrelationSourceState::Active && source.behind == 0)
    }
}

/// Read side of the correlation index. Freshness is computed by the
/// implementation against its sources, never by comparing positions in clients.
#[async_trait]
pub trait CorrelationIndexReader: Send + Sync {
    async fn chain(
        &self,
        correlation_id: &CorrelationId,
    ) -> Result<CorrelationChain, ReadModelError>;

    /// Per-source state and lag, computed against each source stream.
    async fn freshness(&self) -> Result<CorrelationIndexFreshness, ReadModelError>;

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

    fn source(
        context: &str,
        kind: CorrelationSourceKind,
        state: CorrelationSourceState,
        behind: u64,
    ) -> CorrelationSourceFreshness {
        CorrelationSourceFreshness {
            context: context.to_owned(),
            kind,
            stream: format!("{context}-{}", kind.as_str()),
            state,
            behind,
            skipped: 0,
        }
    }

    #[test]
    fn an_inactive_source_hides_the_lag_of_its_context() {
        use CorrelationSourceKind::{Domain, Integration};
        use CorrelationSourceState::{Active, Inactive};
        let freshness = CorrelationIndexFreshness::from_sources(vec![
            source("ordering", Integration, Active, 1),
            source("billing", Domain, Inactive, 0),
            source("ordering", Domain, Active, 2),
            source("billing", Integration, Active, 4),
        ]);
        assert_eq!(
            serde_json::to_value(&freshness.lag).unwrap(),
            serde_json::json!({ "billing": null, "ordering": 3 })
        );
        assert_eq!(freshness.sources[0].context, "billing");
        assert_eq!(freshness.sources[0].kind, Domain);
        assert_eq!(
            serde_json::to_value(&freshness.sources[0]).unwrap(),
            serde_json::json!({ "context": "billing", "kind": "domain",
                "stream": "billing-domain", "state": "inactive", "behind": 0,
                "skipped": 0 })
        );
        assert!(!freshness.is_complete());
    }

    #[test]
    fn only_registered_active_sources_without_lag_are_complete() {
        use CorrelationSourceKind::Domain;
        use CorrelationSourceState::{Active, Inactive};
        assert!(!CorrelationIndexFreshness::default().is_complete());
        assert!(
            CorrelationIndexFreshness::from_sources(vec![source("ordering", Domain, Active, 0)])
                .is_complete()
        );
        assert!(
            !CorrelationIndexFreshness::from_sources(vec![source("ordering", Domain, Active, 1)])
                .is_complete()
        );
        assert!(
            !CorrelationIndexFreshness::from_sources(vec![source("ordering", Domain, Inactive, 0)])
                .is_complete()
        );
    }

    #[test]
    fn page_capacity_divides_the_reference_cap() {
        assert_eq!(
            MAX_CORRELATION_PAGES.checked_mul(CORRELATION_REFS_PER_PAGE),
            Some(MAX_CORRELATION_REFS)
        );
    }
}
