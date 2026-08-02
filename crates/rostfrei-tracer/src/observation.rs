//! A bounded, read-only window of application traffic, independent of operations.
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rostfrei_core::ContentFingerprint;
use rostfrei_messaging_core::ApplicationName;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::{MessageSeriesFidelity, ObservedMessageNode, ObservedMessageSeries};

const MAXIMUM_FLOWS: usize = 128;
const MAXIMUM_FLOW_MESSAGES: usize = 256;
const MAXIMUM_FLOW_BYTES: usize = 256 * 1024;
const MAXIMUM_SOURCES: usize = 32;
const MAXIMUM_SUBSCRIPTIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationScope {
    Test,
    Production,
}

impl ObservationScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogObservation {
    pub application: String,
    pub scope: ObservationScope,
    pub list_href: String,
    pub events_href: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationStatus {
    Connecting,
    Live,
    Unavailable,
    Resetting,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedFlowSummary {
    pub id: String,
    pub correlation_id: String,
    pub name: String,
    pub message_count: usize,
    pub revision: String,
    pub truncated: bool,
    pub conflicted: bool,
    pub detail_href: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationSnapshot {
    pub application: String,
    pub scope: ObservationScope,
    pub generation: String,
    pub revision: String,
    pub status: ObservationStatus,
    pub evicted_flows: String,
    pub discarded_messages: String,
    pub maximum_flows: usize,
    pub items: Vec<ObservedFlowSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedFlow {
    #[serde(flatten)]
    pub summary: ObservedFlowSummary,
    pub application: String,
    pub scope: ObservationScope,
    pub generation: String,
    pub message_series: ObservedMessageSeries,
    pub fidelity: MessageSeriesFidelity,
    /// Continuous observation never establishes business completion.
    pub partial: bool,
}

#[derive(Clone, Debug, Error)]
pub enum ObservationError {
    #[error("continuous observation is not configured for this scope")]
    Unavailable,
    #[error("observed flow is no longer retained; refresh the observation window")]
    Expired,
    #[error("observation source must have a unique, nonempty name of at most 128 bytes")]
    InvalidSource,
    #[error("observation source capacity is exhausted")]
    SourceCapacity,
    #[error("observation subscription capacity is exhausted")]
    SubscriptionCapacity,
}

struct FlowRecord {
    summary: ObservedFlowSummary,
    series: ObservedMessageSeries,
    bytes: usize,
    fingerprints: BTreeMap<String, ContentFingerprint>,
}

struct SourceState {
    identity: u64,
    status: ObservationStatus,
}

struct State {
    generation: String,
    revision: u64,
    next_identity: u64,
    sources: BTreeMap<String, SourceState>,
    flows: VecDeque<FlowRecord>,
    evicted_flows: u64,
    discarded_messages: u64,
    resetting: bool,
    reset_failed: bool,
}

/// One application/scope, with independent retention and observer health.
///
/// Sources must be registered before serving HTTP and marked ready only after
/// broker subscriptions are established. Retained payloads are already redacted.
pub struct ObservationFeed {
    application: ApplicationName,
    scope: ObservationScope,
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    subscriptions: Arc<Semaphore>,
}

impl ObservationFeed {
    pub fn new(application: ApplicationName, scope: ObservationScope) -> Arc<Self> {
        Arc::new(Self {
            application,
            scope,
            state: Mutex::new(State {
                generation: generation(),
                revision: 0,
                next_identity: 0,
                sources: BTreeMap::new(),
                flows: VecDeque::new(),
                evicted_flows: 0,
                discarded_messages: 0,
                resetting: false,
                reset_failed: false,
            }),
            changed: watch::channel(0).0,
            subscriptions: Arc::new(Semaphore::new(MAXIMUM_SUBSCRIPTIONS)),
        })
    }

    pub const fn scope(&self) -> ObservationScope {
        self.scope
    }

    pub fn catalog(&self) -> CatalogObservation {
        let root = format!("/observation/{}", self.scope.as_str());
        CatalogObservation {
            application: self.application.as_str().to_owned(),
            scope: self.scope,
            list_href: root.clone(),
            events_href: format!("{root}/events"),
        }
    }

    pub fn source(self: &Arc<Self>, name: &str) -> Result<ObservationSource, ObservationError> {
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(ObservationError::InvalidSource);
        }
        let mut state = self.lock();
        if let Some(source) = state.sources.get(name) {
            if source.status != ObservationStatus::Unavailable {
                return Err(ObservationError::InvalidSource);
            }
        } else if state.sources.len() >= MAXIMUM_SOURCES {
            return Err(ObservationError::SourceCapacity);
        }
        state.next_identity = state.next_identity.saturating_add(1);
        let identity = state.next_identity;
        state.sources.insert(
            name.to_owned(),
            SourceState {
                identity,
                status: ObservationStatus::Connecting,
            },
        );
        self.notify(&mut state);
        drop(state);
        Ok(ObservationSource {
            feed: Arc::clone(self),
            name: name.to_owned(),
            identity,
        })
    }

    pub fn snapshot(&self) -> ObservationSnapshot {
        let state = self.lock();
        ObservationSnapshot {
            application: self.application.as_str().to_owned(),
            scope: self.scope,
            generation: state.generation.clone(),
            revision: state.revision.to_string(),
            status: status(&state),
            evicted_flows: state.evicted_flows.to_string(),
            discarded_messages: state.discarded_messages.to_string(),
            maximum_flows: MAXIMUM_FLOWS,
            items: state
                .flows
                .iter()
                .rev()
                .map(|flow| flow.summary.clone())
                .collect(),
        }
    }

    pub fn flow(&self, id: &str) -> Result<ObservedFlow, ObservationError> {
        let state = self.lock();
        let flow = state
            .flows
            .iter()
            .find(|flow| flow.summary.id == id)
            .ok_or(ObservationError::Expired)?;
        let complete_identities = !flow.summary.conflicted
            && !flow.summary.truncated
            && !flow.series.messages().is_empty()
            && flow.series.topology_issues().is_empty()
            && flow
                .series
                .messages()
                .iter()
                .all(|message| message.causation_id().is_some());
        Ok(ObservedFlow {
            summary: flow.summary.clone(),
            application: self.application.as_str().to_owned(),
            scope: self.scope,
            generation: state.generation.clone(),
            message_series: flow.series.clone(),
            fidelity: if complete_identities {
                MessageSeriesFidelity::Exact
            } else {
                MessageSeriesFidelity::Grouped
            },
            partial: true,
        })
    }

    pub fn subscribe(self: &Arc<Self>) -> Result<ObservationSubscription, ObservationError> {
        let permit = Arc::clone(&self.subscriptions)
            .try_acquire_owned()
            .map_err(|_| ObservationError::SubscriptionCapacity)?;
        Ok(ObservationSubscription {
            feed: Arc::clone(self),
            changed: self.changed.subscribe(),
            initial: true,
            _permit: permit,
        })
    }

    pub(crate) fn observe(
        &self,
        mut message: ObservedMessageNode,
        policy: &dyn crate::TracePayloadPolicy,
    ) {
        // Validate before admitting a correlation or retaining any caller data.
        let mut validated = ObservedMessageSeries::new();
        if validated.insert_message(message.clone()).is_err() {
            self.discard();
            return;
        }
        drop(validated);
        let Ok(raw) = serde_json::to_vec(&message) else {
            self.discard();
            return;
        };
        let fingerprint = ContentFingerprint::digest(raw);
        let payload = match &mut message {
            ObservedMessageNode::Command { payload, .. }
            | ObservedMessageNode::DomainEvent { payload, .. }
            | ObservedMessageNode::IntegrationEvent { payload, .. } => payload,
        };
        *payload = payload
            .take()
            .and_then(|value| policy.observed_event_payload(value));
        let Ok(bytes) = serde_json::to_vec(&message).map(|value| value.len().saturating_add(96))
        else {
            self.discard();
            return;
        };
        let mut state = self.lock();
        if state.resetting || state.reset_failed {
            return;
        }
        let position = state
            .flows
            .iter()
            .position(|flow| flow.summary.correlation_id == message.correlation_id());
        let mut flow = position
            .and_then(|index| state.flows.remove(index))
            .unwrap_or_else(|| {
                if state.flows.len() == MAXIMUM_FLOWS {
                    state.flows.pop_front();
                    state.evicted_flows = state.evicted_flows.saturating_add(1);
                }
                state.next_identity = state.next_identity.saturating_add(1);
                let id = format!("{}-{}", state.generation, state.next_identity);
                FlowRecord {
                    summary: ObservedFlowSummary {
                        detail_href: format!("/observation/{}/flows/{id}", self.scope.as_str()),
                        id,
                        correlation_id: message.correlation_id().to_owned(),
                        name: message.name().to_owned(),
                        message_count: 0,
                        revision: String::new(),
                        truncated: false,
                        conflicted: false,
                    },
                    series: ObservedMessageSeries::new(),
                    bytes: 0,
                    fingerprints: BTreeMap::new(),
                }
            });
        let existing = flow.series.messages().get(message.message_id()).is_some();
        let identity_conflict = flow
            .fingerprints
            .get(message.message_id())
            .is_some_and(|previous| previous != &fingerprint);
        let changed = if identity_conflict {
            flow.summary.conflicted = true;
            state.discarded_messages = state.discarded_messages.saturating_add(1);
            true
        } else if !existing
            && (flow.series.messages().len() >= MAXIMUM_FLOW_MESSAGES
                || bytes.saturating_add(flow.bytes) > MAXIMUM_FLOW_BYTES)
        {
            flow.summary.truncated = true;
            state.discarded_messages = state.discarded_messages.saturating_add(1);
            true
        } else {
            let message_id = message.message_id().to_owned();
            match flow.series.insert_message(message) {
                Ok(inserted) if inserted.is_duplicate() => false,
                Ok(_) => {
                    flow.bytes = flow.bytes.saturating_add(bytes);
                    flow.fingerprints.insert(message_id, fingerprint);
                    true
                }
                Err(_) => {
                    flow.summary.conflicted = true;
                    state.discarded_messages = state.discarded_messages.saturating_add(1);
                    true
                }
            }
        };
        if changed {
            flow.summary.message_count = flow.series.messages().len();
            flow.summary.revision = state.revision.saturating_add(1).to_string();
            state.flows.push_back(flow);
            self.notify(&mut state);
        } else {
            // Duplicate delivery neither changes recency nor refreshes the graph.
            let position = position.unwrap_or(state.flows.len());
            state.flows.insert(position, flow);
        }
    }

    pub(crate) fn discard(&self) {
        let mut state = self.lock();
        state.discarded_messages = state.discarded_messages.saturating_add(1);
        self.notify(&mut state);
        drop(state);
    }

    pub(crate) fn reset(&self, resetting: bool) {
        let mut state = self.lock();
        state.resetting = resetting;
        state.reset_failed = false;
        state.generation = generation();
        state.flows.clear();
        state.evicted_flows = 0;
        state.discarded_messages = 0;
        self.notify(&mut state);
        drop(state);
    }

    pub(crate) fn fail_reset(&self) {
        let mut state = self.lock();
        state.resetting = false;
        state.reset_failed = true;
        self.notify(&mut state);
        drop(state);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn notify(&self, state: &mut State) {
        state.revision = state.revision.saturating_add(1);
        self.changed.send_replace(state.revision);
    }
}

/// Keep this guard alive for the entire broker observer task. Dropping it makes
/// unavailable coverage visible, including task cancellation and panics.
pub struct ObservationSource {
    feed: Arc<ObservationFeed>,
    name: String,
    identity: u64,
}

impl ObservationSource {
    pub fn ready(&self) {
        self.set_status(ObservationStatus::Live);
    }

    pub fn unavailable(&self) {
        self.set_status(ObservationStatus::Unavailable);
    }

    fn set_status(&self, status: ObservationStatus) {
        let mut state = self.feed.lock();
        if let Some(source) = state.sources.get_mut(&self.name)
            && source.identity == self.identity
            && source.status != status
        {
            source.status = status;
            self.feed.notify(&mut state);
        }
    }
}

impl Drop for ObservationSource {
    fn drop(&mut self) {
        self.set_status(ObservationStatus::Unavailable);
    }
}

pub struct ObservationSubscription {
    _permit: OwnedSemaphorePermit,
    feed: Arc<ObservationFeed>,
    changed: watch::Receiver<u64>,
    initial: bool,
}

impl ObservationSubscription {
    /// Each frame replaces the retained window. Reconnect always sends a fresh
    /// snapshot; no unbounded event queue or historical replay is implied.
    pub async fn next(&mut self) -> Option<ObservationSnapshot> {
        if !self.initial {
            self.changed.changed().await.ok()?;
        }
        self.initial = false;
        self.changed.borrow_and_update();
        Some(self.feed.snapshot())
    }
}

fn status(state: &State) -> ObservationStatus {
    if state.resetting {
        return ObservationStatus::Resetting;
    }
    if state.reset_failed
        || state.sources.is_empty()
        || state
            .sources
            .values()
            .any(|source| source.status == ObservationStatus::Unavailable)
    {
        ObservationStatus::Unavailable
    } else if state
        .sources
        .values()
        .all(|source| source.status == ObservationStatus::Live)
    {
        ObservationStatus::Live
    } else {
        ObservationStatus::Connecting
    }
}

fn generation() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let instant = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{instant}-{}", SEQUENCE.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
#[allow(
    clippy::panic_in_result_fn,
    reason = "test assertions report failures while setup errors use Result"
)]
mod tests {
    use super::*;
    use crate::ExposeTracePayloadsForLocalDevelopment;
    use serde_json::json;

    #[test]
    fn reset_invalidates_the_window_and_failed_reset_blocks_new_evidence()
    -> Result<(), Box<dyn std::error::Error>> {
        let feed = ObservationFeed::new(ApplicationName::new("test-app")?, ObservationScope::Test);
        let source = feed.source("events")?;
        source.ready();
        let event = ObservedMessageNode::domain_event(
            "event",
            "flow",
            Some("external-command".to_owned()),
            "created",
            1,
            None,
            None,
        );
        feed.observe(event.clone(), &ExposeTracePayloadsForLocalDevelopment);
        let before = feed.snapshot();
        let old_id = &before.items.first().ok_or("missing flow")?.id;
        feed.reset(true);
        feed.observe(event.clone(), &ExposeTracePayloadsForLocalDevelopment);
        assert!(feed.snapshot().items.is_empty());
        assert_eq!(feed.snapshot().status, ObservationStatus::Resetting);
        feed.fail_reset();
        feed.observe(event.clone(), &ExposeTracePayloadsForLocalDevelopment);
        assert!(feed.snapshot().items.is_empty());
        assert_eq!(feed.snapshot().status, ObservationStatus::Unavailable);
        feed.reset(false);
        feed.observe(event, &ExposeTracePayloadsForLocalDevelopment);
        assert_eq!(feed.snapshot().status, ObservationStatus::Live);
        assert_ne!(before.generation, feed.snapshot().generation);
        assert!(feed.flow(old_id).is_err());
        Ok(())
    }

    #[test]
    fn oversized_and_long_flows_are_reported_as_truncated() -> Result<(), Box<dyn std::error::Error>>
    {
        let feed = ObservationFeed::new(ApplicationName::new("test-app")?, ObservationScope::Test);
        feed.observe(
            ObservedMessageNode::domain_event(
                "large-event",
                "large",
                None,
                "created",
                1,
                None,
                Some(json!({"text": "a".repeat(MAXIMUM_FLOW_BYTES)})),
            ),
            &ExposeTracePayloadsForLocalDevelopment,
        );
        let snapshot = feed.snapshot();
        let summary = snapshot.items.first().ok_or("missing flow")?;
        assert!(summary.truncated);
        assert_eq!(summary.message_count, 0);
        assert_eq!(
            feed.flow(&summary.id)?.fidelity,
            MessageSeriesFidelity::Grouped
        );
        for index in 0..=MAXIMUM_FLOW_MESSAGES {
            feed.observe(
                ObservedMessageNode::domain_event(
                    format!("event-{index}"),
                    "long",
                    None,
                    "created",
                    1,
                    None,
                    None,
                ),
                &ExposeTracePayloadsForLocalDevelopment,
            );
        }
        let snapshot = feed.snapshot();
        let summary = snapshot.items.first().ok_or("missing flow")?;
        assert!(summary.truncated);
        assert_eq!(summary.message_count, MAXIMUM_FLOW_MESSAGES);
        assert_eq!(snapshot.discarded_messages, "2");
        Ok(())
    }
}
