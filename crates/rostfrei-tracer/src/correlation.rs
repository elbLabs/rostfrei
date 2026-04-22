use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
};

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{watch, Mutex};

use crate::{OperationMode, TracePayloadPolicy};

const DEFAULT_MAXIMUM_CORRELATIONS: usize = 1024;
const DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION: usize = 512;
const DEFAULT_MAXIMUM_BYTES_PER_CORRELATION: usize = 4 * 1024 * 1024;
const MAXIMUM_TOTAL_CORRELATION_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelationEvent {
    pub id: u64,
    pub correlation_id: String,
    #[serde(flatten)]
    pub kind: CorrelationEventKind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum CorrelationEventKind {
    Command {
        operation_id: String,
        command: String,
        schema_version: u32,
        aggregate_type: String,
        aggregate_id: String,
    },
    DomainEvent {
        event_type: String,
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        stream_version: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    IntegrationEvent {
        event_type: String,
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    CommandResult {
        operation_id: String,
        outcome: CorrelationCommandOutcome,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
}

impl CorrelationEventKind {
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::Command { .. } => "command",
            Self::DomainEvent { .. } => "domain-event",
            Self::IntegrationEvent { .. } => "integration-event",
            Self::CommandResult { .. } => "command-result",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CorrelationCommandOutcome {
    Accepted,
    Rejected,
    Failed,
    Indeterminate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomainEventObservation {
    pub event_type: String,
    pub schema_version: u32,
    pub stream_version: Option<u64>,
    pub payload: Option<Value>,
}

impl DomainEventObservation {
    pub fn new(event_type: impl Into<String>, schema_version: u32) -> Self {
        Self {
            event_type: event_type.into(),
            schema_version,
            stream_version: None,
            payload: None,
        }
    }

    #[must_use]
    pub const fn with_stream_version(mut self, stream_version: u64) -> Self {
        self.stream_version = Some(stream_version);
        self
    }

    #[must_use]
    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntegrationEventObservation {
    pub event_type: String,
    pub schema_version: u32,
    pub message_id: Option<String>,
    pub subject: Option<String>,
    pub payload: Option<Value>,
}

impl IntegrationEventObservation {
    pub fn new(event_type: impl Into<String>, schema_version: u32) -> Self {
        Self {
            event_type: event_type.into(),
            schema_version,
            message_id: None,
            subject: None,
            payload: None,
        }
    }

    #[must_use]
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    #[must_use]
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    #[must_use]
    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CorrelationError {
    #[error("invalid correlation identity: {0}")]
    InvalidId(String),
    #[error("correlation was not found")]
    NotFound,
    #[error("correlation capacity is exhausted")]
    CapacityExhausted,
    #[error("correlation event exceeds its retained-byte budget")]
    EventTooLarge,
    #[error("correlation event cursor is ahead of the latest event {latest}")]
    FutureCursor { latest: u64 },
    #[error("correlation event cursor expired; the oldest retained event is {oldest}")]
    ExpiredCursor { oldest: u64 },
}

#[derive(Clone)]
pub struct CorrelationObserver {
    hub: Arc<CorrelationHub>,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
    mode: OperationMode,
}

impl CorrelationObserver {
    pub async fn observe_domain_event(
        &self,
        correlation_id: &str,
        observation: DomainEventObservation,
    ) -> Result<(), CorrelationError> {
        let payload = observation
            .payload
            .and_then(|payload| self.trace_payload_policy.observed_event_payload(payload));
        self.hub
            .observe_for_mode(
                correlation_id,
                self.mode,
                CorrelationEventKind::DomainEvent {
                    event_type: observation.event_type,
                    schema_version: observation.schema_version,
                    stream_version: observation.stream_version,
                    payload,
                },
            )
            .await
    }

    pub async fn observe_integration_event(
        &self,
        correlation_id: &str,
        observation: IntegrationEventObservation,
    ) -> Result<(), CorrelationError> {
        let payload = observation
            .payload
            .and_then(|payload| self.trace_payload_policy.observed_event_payload(payload));
        self.hub
            .observe_for_mode(
                correlation_id,
                self.mode,
                CorrelationEventKind::IntegrationEvent {
                    event_type: observation.event_type,
                    schema_version: observation.schema_version,
                    message_id: observation.message_id,
                    subject: observation.subject,
                    payload,
                },
            )
            .await
    }
}

pub struct CorrelationSubscription {
    record: Arc<CorrelationRecord>,
    receiver: watch::Receiver<u64>,
    cursor: u64,
}

impl CorrelationSubscription {
    pub async fn next(&mut self) -> Option<CorrelationEvent> {
        loop {
            {
                let state = self.record.state.lock().await;
                if let Some(event) = state.events.iter().find(|event| event.id > self.cursor) {
                    self.cursor = event.id;
                    return Some(event.clone());
                }
                if self.record.closed.load(Ordering::Acquire) {
                    return None;
                }
            }
            if self.receiver.changed().await.is_err() {
                return None;
            }
        }
    }
}

pub(crate) struct CorrelationHub {
    state: StdMutex<CorrelationTable>,
    maximum_correlations: usize,
    maximum_events_per_correlation: usize,
    maximum_bytes_per_correlation: usize,
}

impl CorrelationHub {
    pub fn new(maximum_correlations: usize) -> Arc<Self> {
        let maximum_correlations = maximum_correlations.max(1);
        Arc::new(Self {
            state: StdMutex::new(CorrelationTable::default()),
            maximum_correlations,
            maximum_events_per_correlation: DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION,
            maximum_bytes_per_correlation: correlation_byte_budget(maximum_correlations),
        })
    }

    pub fn observer(
        self: &Arc<Self>,
        mode: OperationMode,
        trace_payload_policy: Arc<dyn TracePayloadPolicy>,
    ) -> CorrelationObserver {
        CorrelationObserver {
            hub: Arc::clone(self),
            trace_payload_policy,
            mode,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_command(
        &self,
        correlation_id: &str,
        mode: OperationMode,
        operation_id: String,
        command: String,
        schema_version: u32,
        aggregate_type: String,
        aggregate_id: String,
    ) -> Result<(), CorrelationError> {
        validate_correlation_id(correlation_id)?;
        let mut table = self.state.lock().expect("correlation table lock poisoned");
        if table.records.contains_key(correlation_id) {
            return Err(CorrelationError::InvalidId(
                "correlation is already registered".to_owned(),
            ));
        }
        if table.records.len() >= self.maximum_correlations {
            return Err(CorrelationError::CapacityExhausted);
        }
        let record = CorrelationRecord::new(
            correlation_id.to_owned(),
            mode,
            self.maximum_events_per_correlation,
            self.maximum_bytes_per_correlation,
            CorrelationEventKind::Command {
                operation_id,
                command,
                schema_version,
                aggregate_type,
                aggregate_id,
            },
        )?;
        table.insertion_order.push_back(correlation_id.to_owned());
        table
            .records
            .insert(correlation_id.to_owned(), Arc::clone(&record));
        drop(table);
        Ok(())
    }

    pub async fn command_result(
        &self,
        correlation_id: &str,
        operation_id: String,
        outcome: CorrelationCommandOutcome,
        result: Option<Value>,
    ) -> Result<(), CorrelationError> {
        self.observe(
            correlation_id,
            CorrelationEventKind::CommandResult {
                operation_id,
                outcome,
                result,
            },
        )
        .await
    }

    pub async fn observe(
        &self,
        correlation_id: &str,
        kind: CorrelationEventKind,
    ) -> Result<(), CorrelationError> {
        validate_correlation_id(correlation_id)?;
        let record = self.record(correlation_id)?;
        record.append(kind).await
    }

    async fn observe_for_mode(
        &self,
        correlation_id: &str,
        mode: OperationMode,
        kind: CorrelationEventKind,
    ) -> Result<(), CorrelationError> {
        validate_correlation_id(correlation_id)?;
        let record = self.record(correlation_id)?;
        if record.mode != mode {
            return Err(CorrelationError::InvalidId(
                "correlation does not belong to the observer environment".to_owned(),
            ));
        }
        record.append(kind).await
    }

    pub fn mode(&self, correlation_id: &str) -> Result<OperationMode, CorrelationError> {
        Ok(self.record(correlation_id)?.mode)
    }

    pub async fn subscribe(
        &self,
        correlation_id: &str,
        after: u64,
    ) -> Result<CorrelationSubscription, CorrelationError> {
        self.subscribe_with_mode(correlation_id, after)
            .await
            .map(|(_, subscription)| subscription)
    }

    pub async fn subscribe_with_mode(
        &self,
        correlation_id: &str,
        after: u64,
    ) -> Result<(OperationMode, CorrelationSubscription), CorrelationError> {
        let record = self.record(correlation_id)?;
        let mode = record.mode;
        let state = record.state.lock().await;
        let latest = state.next_id.saturating_sub(1);
        let oldest = state.events.front().map_or(state.next_id, |event| event.id);
        if after > latest {
            return Err(CorrelationError::FutureCursor { latest });
        }
        if after.saturating_add(1) < oldest {
            return Err(CorrelationError::ExpiredCursor { oldest });
        }
        drop(state);
        Ok((
            mode,
            CorrelationSubscription {
                receiver: record.changed.subscribe(),
                record,
                cursor: after,
            },
        ))
    }

    pub fn retain_dispatch_correlations(&self) {
        let mut table = self.state.lock().expect("correlation table lock poisoned");
        let removed = table
            .records
            .extract_if(|_, record| record.mode != OperationMode::Dispatch)
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        let retained = table.records.keys().cloned().collect::<HashSet<_>>();
        table
            .insertion_order
            .retain(|correlation_id| retained.contains(correlation_id));
        drop(table);
        for record in removed {
            record.close();
        }
    }

    pub fn remove(&self, correlation_id: &str) {
        let record = {
            let mut table = self.state.lock().expect("correlation table lock poisoned");
            table
                .insertion_order
                .retain(|retained| retained != correlation_id);
            table.records.remove(correlation_id)
        };
        if let Some(record) = record {
            record.close();
        }
    }

    fn record(&self, correlation_id: &str) -> Result<Arc<CorrelationRecord>, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        self.state
            .lock()
            .expect("correlation table lock poisoned")
            .records
            .get(correlation_id)
            .cloned()
            .ok_or(CorrelationError::NotFound)
    }
}

impl Default for CorrelationHub {
    fn default() -> Self {
        Self {
            state: StdMutex::new(CorrelationTable::default()),
            maximum_correlations: DEFAULT_MAXIMUM_CORRELATIONS,
            maximum_events_per_correlation: DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION,
            maximum_bytes_per_correlation: correlation_byte_budget(DEFAULT_MAXIMUM_CORRELATIONS),
        }
    }
}

#[derive(Default)]
struct CorrelationTable {
    records: HashMap<String, Arc<CorrelationRecord>>,
    insertion_order: VecDeque<String>,
}

struct CorrelationRecord {
    correlation_id: String,
    mode: OperationMode,
    maximum_events: usize,
    maximum_bytes: usize,
    state: Mutex<CorrelationState>,
    lifecycle: StdMutex<()>,
    changed: watch::Sender<u64>,
    closed: AtomicBool,
}

impl CorrelationRecord {
    fn new(
        correlation_id: String,
        mode: OperationMode,
        maximum_events: usize,
        maximum_bytes: usize,
        initial: CorrelationEventKind,
    ) -> Result<Arc<Self>, CorrelationError> {
        let event = bounded_event(
            CorrelationEvent {
                id: 1,
                correlation_id: correlation_id.clone(),
                kind: initial,
            },
            maximum_bytes,
        )?;
        let retained_bytes = serialized_event_len(&event);
        let (changed, _) = watch::channel(1);
        Ok(Arc::new(Self {
            correlation_id,
            mode,
            maximum_events,
            maximum_bytes,
            state: Mutex::new(CorrelationState {
                next_id: 2,
                events: VecDeque::from([event]),
                retained_bytes,
            }),
            lifecycle: StdMutex::new(()),
            changed,
            closed: AtomicBool::new(false),
        }))
    }

    async fn append(&self, kind: CorrelationEventKind) -> Result<(), CorrelationError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let mut state = self.state.lock().await;
        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("correlation lifecycle lock poisoned");
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let id = state.next_id;
        state.next_id = state
            .next_id
            .checked_add(1)
            .expect("bounded correlation events cannot exhaust u64 IDs");
        let event = bounded_event(
            CorrelationEvent {
                id,
                correlation_id: self.correlation_id.clone(),
                kind,
            },
            self.maximum_bytes,
        )?;
        state.retained_bytes = state
            .retained_bytes
            .saturating_add(serialized_event_len(&event));
        state.events.push_back(event);
        while state.events.len() > self.maximum_events || state.retained_bytes > self.maximum_bytes
        {
            let Some(removed) = state.events.pop_front() else {
                break;
            };
            state.retained_bytes = state
                .retained_bytes
                .saturating_sub(serialized_event_len(&removed));
        }
        drop(state);
        self.changed.send_replace(id);
        Ok(())
    }

    fn close(&self) {
        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("correlation lifecycle lock poisoned");
        self.closed.store(true, Ordering::Release);
        let latest = *self.changed.borrow();
        self.changed.send_replace(latest);
    }
}

struct CorrelationState {
    next_id: u64,
    events: VecDeque<CorrelationEvent>,
    retained_bytes: usize,
}

fn serialized_event_len(event: &CorrelationEvent) -> usize {
    serde_json::to_vec(event)
        .expect("correlation events always serialize")
        .len()
}

fn correlation_byte_budget(maximum_correlations: usize) -> usize {
    (MAXIMUM_TOTAL_CORRELATION_BYTES / maximum_correlations.max(1))
        .min(DEFAULT_MAXIMUM_BYTES_PER_CORRELATION)
}

fn bounded_event(
    mut event: CorrelationEvent,
    maximum_bytes: usize,
) -> Result<CorrelationEvent, CorrelationError> {
    if serialized_event_len(&event) <= maximum_bytes {
        return Ok(event);
    }
    match &mut event.kind {
        CorrelationEventKind::DomainEvent { payload, .. }
        | CorrelationEventKind::IntegrationEvent { payload, .. } => *payload = None,
        CorrelationEventKind::CommandResult { result, .. } => *result = None,
        CorrelationEventKind::Command { .. } => return Err(CorrelationError::EventTooLarge),
    }
    if serialized_event_len(&event) <= maximum_bytes {
        Ok(event)
    } else {
        Err(CorrelationError::EventTooLarge)
    }
}

pub(crate) fn validate_correlation_id(value: &str) -> Result<(), CorrelationError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(CorrelationError::InvalidId(
            "correlation ID must contain 1-256 non-control characters".to_owned(),
        ));
    }
    Ok(())
}
