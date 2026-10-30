use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use rostfrei_core::{
    AggregateId, AggregateType, CorrelationChain, CorrelationHistory, CorrelationRef,
    CorrelationSnapshot, EventHistory, EventStoreError, EventStoreErrorKind, RecordedEvent,
    StreamId,
};
use rostfrei_messaging_core::{BoundedContextName, CorrelationId, MessageTimestamp};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::TracePayloadPolicy;

/// Correlations returned for one anchor; the most recently started are kept.
pub const MAX_ANCHOR_CORRELATIONS: usize = 256;
pub const MAX_INCLUDED_CONTEXTS: usize = 16;
/// Each included stream requires a full replay.
pub const MAX_INCLUDED_STREAMS: usize = 64;
pub const MAX_PAYLOAD_MATCHES: usize = 8;
/// Measured after URL decoding.
pub const MAX_PAYLOAD_MATCH_VALUE_BYTES: usize = 256;
pub const MAX_PAYLOAD_MATCH_FIELD_SEGMENTS: usize = 8;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CorrelationInspectionError {
    #[error("retained correlation history is not configured")]
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
    #[error("correlation `{0}` has no retained references")]
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
    #[error("retained correlation read failed: {0}")]
    Read(EventStoreError),
    #[error("event history read failed: {0}")]
    History(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnchorIdentity {
    pub aggregate_type: String,
    pub aggregate_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnchorCorrelations {
    pub anchor: AnchorIdentity,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matched: Vec<PayloadMatch>,
    pub snapshot: CorrelationSnapshot,
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
pub struct RetainedCorrelation {
    pub correlation_id: String,
    pub truncated: bool,
    pub snapshot: CorrelationSnapshot,
    pub refs: Vec<CorrelationRefView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CorrelationRefView {
    Domain(DomainRefView),
    Integration(rostfrei_core::IntegrationEventRef),
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

/// Matches only loaded, policy-filtered domain payloads. Nulls, objects, arrays,
/// missing fields and integration references never match.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PayloadMatch {
    pub context: String,
    pub event_type: String,
    pub field: String,
    pub value: String,
}

impl PayloadMatch {
    /// Parses `<context>/<event-type>.<field>[.<field>…]=<value>`. The value is
    /// everything after the first `=`. A numeric segment indexes into an array.
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

pub struct CorrelationInspection {
    reader: Arc<dyn CorrelationHistory>,
    histories: BTreeMap<String, Arc<dyn EventHistory>>,
    anchors: BTreeSet<String>,
}

type Payloads = HashMap<(String, String), Value>;

impl CorrelationInspection {
    pub(crate) const fn new(
        reader: Arc<dyn CorrelationHistory>,
        histories: BTreeMap<String, Arc<dyn EventHistory>>,
        anchors: BTreeSet<String>,
    ) -> Self {
        Self {
            reader,
            histories,
            anchors,
        }
    }

    pub(crate) async fn anchor(
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
        let mut seen = BTreeSet::new();
        let mut correlations = Vec::new();
        for event in &events {
            if let Some(id) = event.correlation_id()
                && seen.insert(id.as_str())
            {
                correlations.push(id.clone());
            }
        }
        let skipped = correlations.len().saturating_sub(MAX_ANCHOR_CORRELATIONS);
        let selected: Vec<_> = correlations.into_iter().skip(skipped).collect();
        let read = self
            .reader
            .load_correlations(&selected)
            .await
            .map_err(CorrelationInspectionError::Read)?;
        let truncated = skipped > 0 || read.chains.iter().any(|chain| chain.truncated);
        let mut by_id: HashMap<_, _> = read
            .chains
            .into_iter()
            .map(|chain| (chain.correlation_id.clone(), chain))
            .collect();
        let chains: Vec<_> = selected
            .iter()
            .map(|id| {
                by_id
                    .remove(id.as_str())
                    .unwrap_or_else(|| CorrelationChain {
                        correlation_id: id.as_str().to_owned(),
                        ..CorrelationChain::default()
                    })
            })
            .collect();
        let events = refresh_anchor_history(history.as_ref(), &stream, events, &chains).await?;
        let mut payloads = Payloads::new();
        for event in events {
            if let Some(payload) = decoded_payload(event.payload(), policy) {
                payloads.insert(
                    (context.to_owned(), event.event_id().as_str().to_owned()),
                    payload,
                );
            }
        }
        self.load_included(&stream, include, &chains, policy, &mut payloads)
            .await?;
        let correlations = chains
            .into_iter()
            .filter(|chain| {
                matches
                    .iter()
                    .all(|filter| filter.holds_for(chain, &payloads))
            })
            .map(|chain| AnchoredCorrelation {
                correlation_id: chain.correlation_id.clone(),
                truncated: chain.truncated,
                refs: views(chain, &payloads),
            })
            .collect();
        Ok(AnchorCorrelations {
            anchor: AnchorIdentity {
                aggregate_type: aggregate_type.to_owned(),
                aggregate_id: aggregate_id.to_owned(),
            },
            matched: matches.to_vec(),
            snapshot: read.snapshot,
            correlations,
            truncated,
        })
    }

    pub(crate) async fn correlation(
        &self,
        correlation_id: &str,
    ) -> Result<RetainedCorrelation, CorrelationInspectionError> {
        let correlation = CorrelationId::new(correlation_id)
            .map_err(|error| CorrelationInspectionError::InvalidId(error.to_string()))?;
        let read = self
            .reader
            .load_correlations(&[correlation])
            .await
            .map_err(CorrelationInspectionError::Read)?;
        let chain = read
            .chains
            .into_iter()
            .find(|chain| chain.correlation_id == correlation_id)
            .unwrap_or_default();
        if chain.refs.is_empty() && !chain.truncated {
            return Err(CorrelationInspectionError::NotFound(
                correlation_id.to_owned(),
            ));
        }
        Ok(RetainedCorrelation {
            correlation_id: correlation_id.to_owned(),
            truncated: chain.truncated,
            snapshot: read.snapshot,
            refs: views(chain, &Payloads::new()),
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
        chains: &[CorrelationChain],
        policy: &dyn TracePayloadPolicy,
        payloads: &mut Payloads,
    ) -> Result<(), CorrelationInspectionError> {
        let mut streams = BTreeSet::new();
        for reference in chains.iter().flat_map(|chain| &chain.refs) {
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
}

// Correlation cutoffs can follow the first anchor load. Refresh only when a
// returned anchor reference is newer, keeping the original refs and cutoffs.
async fn refresh_anchor_history(
    history: &dyn EventHistory,
    anchor: &StreamId,
    initial: Vec<RecordedEvent>,
    chains: &[CorrelationChain],
) -> Result<Vec<RecordedEvent>, CorrelationInspectionError> {
    let version = initial
        .last()
        .map_or(0, |event| event.stream_version().value());
    let references: Vec<_> = chains
        .iter()
        .flat_map(|chain| &chain.refs)
        .filter_map(|reference| match reference {
            CorrelationRef::Domain(reference)
                if reference.aggregate_type == anchor.aggregate_type().as_str()
                    && reference.aggregate_id == anchor.aggregate_id().as_str() =>
            {
                Some(reference)
            }
            _ => None,
        })
        .collect();
    if !references
        .iter()
        .any(|reference| reference.stream_version > version)
    {
        return Ok(initial);
    }
    let events = history
        .load(anchor)
        .await
        .map_err(|error| CorrelationInspectionError::History(error.to_string()))?;
    let recorded: BTreeSet<_> = events
        .iter()
        .filter(|event| event.stream_id() == anchor)
        .map(|event| (event.event_id().as_str(), event.stream_version().value()))
        .collect();
    if let Some(missing) = references.iter().find(|reference| {
        !recorded.contains(&(reference.event_id.as_str(), reference.stream_version))
    }) {
        return Err(CorrelationInspectionError::Read(EventStoreError::new(
            EventStoreErrorKind::CorruptHistory,
            format!(
                "anchor history is missing retained event `{}` at stream version {} after reload",
                missing.event_id, missing.stream_version
            ),
        )));
    }
    Ok(events)
}

fn views(chain: CorrelationChain, payloads: &Payloads) -> Vec<CorrelationRefView> {
    chain
        .refs
        .into_iter()
        .map(|reference| match reference {
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
            CorrelationRef::Integration(reference) => CorrelationRefView::Integration(reference),
        })
        .collect()
}

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
