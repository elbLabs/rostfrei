use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use rostfrei_core::{
    AggregateId, AggregateType, CorrelationChain, CorrelationIndexFreshness,
    CorrelationIndexReader, CorrelationRef, EventHistory, ReadModelError, StreamId,
};
use rostfrei_messaging_core::{BoundedContextName, CorrelationId, MessageTimestamp};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::TracePayloadPolicy;

/// Correlations returned for one anchor; the most recently started are kept.
pub const MAX_ANCHOR_CORRELATIONS: usize = 256;
pub const MAX_INCLUDED_CONTEXTS: usize = 16;
/// Foreign streams loaded for `include`; each load replays a whole stream.
pub const MAX_INCLUDED_STREAMS: usize = 64;
/// `match` filters accepted by one anchored view; all of them must hold.
pub const MAX_PAYLOAD_MATCHES: usize = 8;
/// Longest `match` value, in bytes after URL decoding.
pub const MAX_PAYLOAD_MATCH_VALUE_BYTES: usize = 256;
/// Deepest `match` field path, in dot-separated segments.
pub const MAX_PAYLOAD_MATCH_FIELD_SEGMENTS: usize = 8;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CorrelationInspectionError {
    #[error("the correlation index is not configured")]
    Unavailable,
    #[error("`{0}` is not a declared anchor aggregate type")]
    UnknownAnchor(String),
    #[error("`{aggregate_type}` has no stream `{aggregate_id}`; anchor IDs are aggregate IDs")]
    UnknownAnchorStream {
        aggregate_type: String,
        aggregate_id: String,
    },
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
    #[error("invalid match: {0}")]
    InvalidMatch(String),
    #[error(
        "payloads of context `{0}` are not loaded for this view; add `include=context:{0}` to match them"
    )]
    MatchRequiresInclude(String),
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
    /// The applied `match` filters; omitted when the view is unfiltered.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matched: Vec<PayloadMatch>,
    /// Serialized as top-level `lag` and `sources`.
    #[serde(flatten)]
    pub freshness: CorrelationIndexFreshness,
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
    /// Serialized as top-level `lag` and `sources`.
    #[serde(flatten)]
    pub freshness: CorrelationIndexFreshness,
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

/// A read-time filter on an anchored view: keeps a correlation when one of its
/// domain references of `context` and `event_type` has a loaded payload whose
/// value at the dot path `field` equals `value`.
///
/// The value is compared with the payload field's JSON type: as text for a
/// string, numerically for a number and as `true`/`false` for a boolean. Null,
/// objects, arrays, missing fields and integration references never match.
/// Payloads are matched as the [`TracePayloadPolicy`] returns them, so a hidden
/// payload or field never matches.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PayloadMatch {
    pub context: String,
    pub event_type: String,
    pub field: String,
    pub value: String,
}

impl PayloadMatch {
    /// Parses `<context>/<event-type>.<field>[.<field>…]=<value>`. The value is
    /// everything after the first `=` and may itself contain `=`. A numeric
    /// segment indexes into an array.
    pub fn parse(expression: &str) -> Result<Self, CorrelationInspectionError> {
        let invalid = |reason: &str| {
            CorrelationInspectionError::InvalidMatch(format!(
                "`{expression}`: {reason}; use `<context>/<event-type>.<field>=<value>`"
            ))
        };
        let (path, value) = expression
            .split_once('=')
            .ok_or_else(|| invalid("missing `=<value>`"))?;
        let (context, event) = path
            .split_once('/')
            .ok_or_else(|| invalid("missing `<context>/`"))?;
        let context =
            BoundedContextName::new(context).map_err(|_| invalid("invalid context name"))?;
        let (event_type, field) = event
            .split_once('.')
            .ok_or_else(|| invalid("missing `.<field>`"))?;
        if event_type.is_empty() || event_type.contains('/') {
            return Err(invalid("invalid event type"));
        }
        if field.split('.').any(str::is_empty) {
            return Err(invalid("empty field path segment"));
        }
        if field.split('.').count() > MAX_PAYLOAD_MATCH_FIELD_SEGMENTS {
            return Err(invalid(&format!(
                "field paths have at most {MAX_PAYLOAD_MATCH_FIELD_SEGMENTS} segments"
            )));
        }
        if value.len() > MAX_PAYLOAD_MATCH_VALUE_BYTES {
            return Err(invalid(&format!(
                "values have at most {MAX_PAYLOAD_MATCH_VALUE_BYTES} bytes"
            )));
        }
        Ok(Self {
            context: context.as_str().to_owned(),
            event_type: event_type.to_owned(),
            field: field.to_owned(),
            value: value.to_owned(),
        })
    }

    fn holds_for(&self, chain: &CorrelationChain, payloads: &Payloads) -> bool {
        chain.refs.iter().any(|reference| {
            let CorrelationRef::Domain(reference) = reference else {
                return false;
            };
            reference.context == self.context
                && reference.event_type == self.event_type
                && payloads
                    .get(&(reference.context.clone(), reference.event_id.clone()))
                    .and_then(|payload| self.field_of(payload))
                    .is_some_and(|field| self.equals(field))
        })
    }

    fn field_of<'a>(&self, payload: &'a Value) -> Option<&'a Value> {
        self.field
            .split('.')
            .try_fold(payload, |value, segment| match value {
                Value::Object(fields) => fields.get(segment),
                Value::Array(items) => segment
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| items.get(index)),
                _ => None,
            })
    }

    fn equals(&self, field: &Value) -> bool {
        match field {
            Value::String(text) => *text == self.value,
            Value::Bool(flag) => self.value.parse::<bool>().is_ok_and(|value| value == *flag),
            Value::Number(number) => self
                .value
                .parse::<serde_json::Number>()
                .is_ok_and(|value| numbers_equal(number, &value)),
            Value::Null | Value::Array(_) | Value::Object(_) => false,
        }
    }
}

fn numbers_equal(left: &serde_json::Number, right: &serde_json::Number) -> bool {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_u64(), right.as_u64()) {
        return left == right;
    }
    left.as_f64()
        .zip(right.as_f64())
        .is_some_and(|(left, right)| left.total_cmp(&right).is_eq())
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
        matches: &[PayloadMatch],
        policy: &dyn TracePayloadPolicy,
    ) -> Result<AnchorCorrelations, CorrelationInspectionError> {
        if !self.anchors.contains(aggregate_type) {
            return Err(CorrelationInspectionError::UnknownAnchor(
                aggregate_type.to_owned(),
            ));
        }
        let context = aggregate_type
            .split_once('/')
            .map_or(aggregate_type, |(context, _)| context);
        check_view_request(context, include, matches)?;
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
        if events.is_empty() {
            return Err(CorrelationInspectionError::UnknownAnchorStream {
                aggregate_type: aggregate_type.to_owned(),
                aggregate_id: aggregate_id.to_owned(),
            });
        }
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
        let freshness = self
            .index
            .freshness()
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
            if !matches
                .iter()
                .all(|filter| filter.holds_for(&chain, &payloads))
            {
                continue;
            }
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
            matched: matches.to_vec(),
            freshness,
            correlations: anchored,
            truncated: skipped > 0,
        })
    }

    /// `NotFound` only when the index holds no reference and is complete: every
    /// registered source has a running projector and none is behind. Otherwise
    /// an empty chain is returned with its freshness.
    pub async fn correlation(
        &self,
        correlation_id: &str,
    ) -> Result<IndexedCorrelation, CorrelationInspectionError> {
        let correlation = CorrelationId::new(correlation_id)
            .map_err(|error| CorrelationInspectionError::InvalidId(error.to_string()))?;
        let freshness = self
            .index
            .freshness()
            .await
            .map_err(CorrelationInspectionError::Index)?;
        let chain = self
            .index
            .chain(&correlation)
            .await
            .map_err(CorrelationInspectionError::Index)?;
        if chain.refs.is_empty() && freshness.is_complete() {
            return Err(CorrelationInspectionError::NotFound(
                correlation_id.to_owned(),
            ));
        }
        Ok(IndexedCorrelation {
            correlation_id: correlation_id.to_owned(),
            truncated: chain.truncated,
            freshness,
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

/// Rejects a request whose `match` filters name a context whose payloads the
/// view does not load; it would otherwise filter out every correlation.
fn check_view_request(
    anchor_context: &str,
    include: &BTreeSet<String>,
    matches: &[PayloadMatch],
) -> Result<(), CorrelationInspectionError> {
    if include.len() > MAX_INCLUDED_CONTEXTS {
        return Err(CorrelationInspectionError::IncludeTooBroad {
            subject: "contexts",
            maximum: MAX_INCLUDED_CONTEXTS,
        });
    }
    if matches.len() > MAX_PAYLOAD_MATCHES {
        return Err(CorrelationInspectionError::InvalidMatch(format!(
            "at most {MAX_PAYLOAD_MATCHES} match parameters are accepted"
        )));
    }
    matches
        .iter()
        .find(|filter| filter.context != anchor_context && !include.contains(&filter.context))
        .map_or(Ok(()), |unloaded| {
            Err(CorrelationInspectionError::MatchRequiresInclude(
                unloaded.context.clone(),
            ))
        })
}

fn decoded_payload(payload: &[u8], policy: &dyn TracePayloadPolicy) -> Option<Value> {
    serde_json::from_slice(payload)
        .ok()
        .and_then(|payload| policy.observed_event_payload(payload))
}
