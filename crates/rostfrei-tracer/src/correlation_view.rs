use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use rostfrei_core::{
    AggregateId, AggregateType, CorrelationChain, CorrelationIndexLag, CorrelationIndexReader,
    CorrelationRef, EventHistory, ReadModelError, StreamId,
};
use rostfrei_messaging_core::{CorrelationId, MessageTimestamp};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::TracePayloadPolicy;

/// Correlations returned for one anchor; the most recently started are kept.
pub const MAX_ANCHOR_CORRELATIONS: usize = 256;
pub const MAX_INCLUDED_CONTEXTS: usize = 16;
/// Foreign streams loaded for `include`; each load replays a whole stream.
pub const MAX_INCLUDED_STREAMS: usize = 64;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CorrelationInspectionError {
    #[error("the correlation index is not configured")]
    Unavailable,
    #[error("`{0}` is not a declared anchor aggregate type")]
    UnknownAnchor(String),
    #[error("invalid identifier: {0}")]
    InvalidId(String),
    #[error("correlation `{0}` is not indexed")]
    NotFound(String),
    #[error("no production event history is configured for context `{0}`")]
    HistoryUnavailable(String),
    #[error("at most {maximum} {subject} can be included in one request")]
    IncludeTooBroad {
        subject: &'static str,
        maximum: usize,
    },
    #[error("correlation index read failed: {0}")]
    Index(ReadModelError),
    #[error("event history read failed: {0}")]
    History(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnchorIdentity {
    pub aggregate_type: String,
    pub aggregate_id: String,
}

/// The anchor's correlations, each expanded from the correlation index alone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnchorCorrelations {
    pub anchor: AnchorIdentity,
    pub lag: CorrelationIndexLag,
    pub correlations: Vec<AnchoredCorrelation>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnchoredCorrelation {
    pub correlation_id: String,
    pub truncated: bool,
    pub refs: Vec<CorrelationRefView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexedCorrelation {
    pub correlation_id: String,
    pub truncated: bool,
    pub lag: CorrelationIndexLag,
    pub refs: Vec<CorrelationRefView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CorrelationRefView {
    Domain(DomainRefView),
    Integration(IntegrationRefView),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DomainRefView {
    pub context: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub stream_version: u64,
    pub event_id: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub occurred_at: MessageTimestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntegrationRefView {
    pub context: String,
    pub subject: String,
    pub message_id: String,
    pub event_type: String,
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub occurred_at: MessageTimestamp,
    pub available: bool,
}

/// Production correlation inspection over the durable index (ADR 0045).
pub struct CorrelationInspection {
    index: Arc<dyn CorrelationIndexReader>,
    histories: BTreeMap<String, Arc<dyn EventHistory>>,
    anchors: BTreeSet<String>,
}

type Payloads = HashMap<(String, String), Value>;

impl CorrelationInspection {
    pub const fn new(
        index: Arc<dyn CorrelationIndexReader>,
        histories: BTreeMap<String, Arc<dyn EventHistory>>,
        anchors: BTreeSet<String>,
    ) -> Self {
        Self {
            index,
            histories,
            anchors,
        }
    }

    pub async fn anchor(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        include: &BTreeSet<String>,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<AnchorCorrelations, CorrelationInspectionError> {
        if !self.anchors.contains(aggregate_type) {
            return Err(CorrelationInspectionError::UnknownAnchor(
                aggregate_type.to_owned(),
            ));
        }
        if include.len() > MAX_INCLUDED_CONTEXTS {
            return Err(CorrelationInspectionError::IncludeTooBroad {
                subject: "contexts",
                maximum: MAX_INCLUDED_CONTEXTS,
            });
        }
        let context = aggregate_type
            .split_once('/')
            .map_or(aggregate_type, |(context, _)| context);
        let history = self.history(context)?;
        for included in include {
            self.history(included)?;
        }
        let stream = StreamId::new(
            AggregateType::new(aggregate_type)
                .map_err(|error| CorrelationInspectionError::InvalidId(error.to_string()))?,
            AggregateId::new(aggregate_id)
                .map_err(|error| CorrelationInspectionError::InvalidId(error.to_string()))?,
        );
        let events = history
            .load(&stream)
            .await
            .map_err(|error| CorrelationInspectionError::History(error.to_string()))?;
        let mut payloads = Payloads::new();
        let mut correlations: Vec<CorrelationId> = Vec::new();
        for event in &events {
            if let Some(payload) = decoded_payload(event.payload(), policy) {
                payloads.insert(
                    (context.to_owned(), event.event_id().as_str().to_owned()),
                    payload,
                );
            }
            if let Some(correlation_id) = event.correlation_id()
                && !correlations.contains(correlation_id)
            {
                correlations.push(correlation_id.clone());
            }
        }
        let skipped = correlations.len().saturating_sub(MAX_ANCHOR_CORRELATIONS);
        let lag = self
            .index
            .lag()
            .await
            .map_err(CorrelationInspectionError::Index)?;
        let mut chains = Vec::new();
        for correlation_id in correlations.into_iter().skip(skipped) {
            let chain = self
                .index
                .chain(&correlation_id)
                .await
                .map_err(CorrelationInspectionError::Index)?;
            chains.push((correlation_id, chain));
        }
        self.load_included(&stream, include, &chains, policy, &mut payloads)
            .await?;
        let mut anchored = Vec::new();
        for (correlation_id, chain) in chains {
            anchored.push(AnchoredCorrelation {
                correlation_id: correlation_id.as_str().to_owned(),
                truncated: chain.truncated,
                refs: self.views(chain, &payloads).await?,
            });
        }
        Ok(AnchorCorrelations {
            anchor: AnchorIdentity {
                aggregate_type: aggregate_type.to_owned(),
                aggregate_id: aggregate_id.to_owned(),
            },
            lag,
            correlations: anchored,
            truncated: skipped > 0,
        })
    }

    pub async fn correlation(
        &self,
        correlation_id: &str,
    ) -> Result<IndexedCorrelation, CorrelationInspectionError> {
        let correlation = CorrelationId::new(correlation_id)
            .map_err(|error| CorrelationInspectionError::InvalidId(error.to_string()))?;
        let lag = self
            .index
            .lag()
            .await
            .map_err(CorrelationInspectionError::Index)?;
        let chain = self
            .index
            .chain(&correlation)
            .await
            .map_err(CorrelationInspectionError::Index)?;
        if chain.refs.is_empty() && lag.values().all(|behind| *behind == 0) {
            return Err(CorrelationInspectionError::NotFound(
                correlation_id.to_owned(),
            ));
        }
        Ok(IndexedCorrelation {
            correlation_id: correlation_id.to_owned(),
            truncated: chain.truncated,
            lag,
            refs: self.views(chain, &Payloads::new()).await?,
        })
    }

    fn history(&self, context: &str) -> Result<&Arc<dyn EventHistory>, CorrelationInspectionError> {
        self.histories
            .get(context)
            .ok_or_else(|| CorrelationInspectionError::HistoryUnavailable(context.to_owned()))
    }

    async fn load_included(
        &self,
        anchor: &StreamId,
        include: &BTreeSet<String>,
        chains: &[(CorrelationId, CorrelationChain)],
        policy: &dyn TracePayloadPolicy,
        payloads: &mut Payloads,
    ) -> Result<(), CorrelationInspectionError> {
        let mut streams = BTreeSet::new();
        for reference in chains.iter().flat_map(|(_, chain)| &chain.refs) {
            let CorrelationRef::Domain(reference) = reference else {
                continue;
            };
            if !include.contains(&reference.context) {
                continue;
            }
            let stream = StreamId::new(
                AggregateType::new(reference.aggregate_type.as_str())
                    .map_err(|error| CorrelationInspectionError::InvalidId(error.to_string()))?,
                AggregateId::new(reference.aggregate_id.as_str())
                    .map_err(|error| CorrelationInspectionError::InvalidId(error.to_string()))?,
            );
            if &stream != anchor {
                streams.insert((reference.context.clone(), stream));
            }
        }
        if streams.len() > MAX_INCLUDED_STREAMS {
            return Err(CorrelationInspectionError::IncludeTooBroad {
                subject: "streams",
                maximum: MAX_INCLUDED_STREAMS,
            });
        }
        for (context, stream) in streams {
            let events = self
                .history(&context)?
                .load(&stream)
                .await
                .map_err(|error| CorrelationInspectionError::History(error.to_string()))?;
            for event in events {
                if let Some(payload) = decoded_payload(event.payload(), policy) {
                    payloads.insert(
                        (context.clone(), event.event_id().as_str().to_owned()),
                        payload,
                    );
                }
            }
        }
        Ok(())
    }

    async fn views(
        &self,
        chain: CorrelationChain,
        payloads: &Payloads,
    ) -> Result<Vec<CorrelationRefView>, CorrelationInspectionError> {
        let mut views = Vec::with_capacity(chain.refs.len());
        for reference in chain.refs {
            views.push(match reference {
                CorrelationRef::Domain(reference) => {
                    let payload = payloads
                        .get(&(reference.context.clone(), reference.event_id.clone()))
                        .cloned();
                    CorrelationRefView::Domain(DomainRefView {
                        context: reference.context,
                        aggregate_type: reference.aggregate_type,
                        aggregate_id: reference.aggregate_id,
                        stream_version: reference.stream_version,
                        event_id: reference.event_id,
                        event_type: reference.event_type,
                        causation_id: reference.causation_id,
                        occurred_at: reference.occurred_at,
                        payload,
                    })
                }
                CorrelationRef::Integration(reference) => {
                    let available = self
                        .index
                        .integration_available(&reference)
                        .await
                        .map_err(CorrelationInspectionError::Index)?;
                    CorrelationRefView::Integration(IntegrationRefView {
                        context: reference.context,
                        subject: reference.subject,
                        message_id: reference.message_id,
                        event_type: reference.event_type,
                        schema_version: reference.schema_version,
                        causation_id: reference.causation_id,
                        occurred_at: reference.occurred_at,
                        available,
                    })
                }
            });
        }
        Ok(views)
    }
}

fn decoded_payload(payload: &[u8], policy: &dyn TracePayloadPolicy) -> Option<Value> {
    serde_json::from_slice(payload)
        .ok()
        .and_then(|payload| policy.observed_event_payload(payload))
}
