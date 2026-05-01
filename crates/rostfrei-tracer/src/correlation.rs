use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex as StdMutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, watch};

use rostfrei_messaging_core::{CommandResponseOutcome, MessageSeriesInsertOutcome};

use crate::{
    ObservedCommandOutcome, ObservedMessageNode, ObservedMessageSeries, ObservedMessageSeriesError,
    OperationMode, TestAggregate, TracePayloadPolicy,
};

const DEFAULT_MAXIMUM_CORRELATIONS: usize = 1024;
const DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION: usize = 512;
const DEFAULT_MAXIMUM_BYTES_PER_CORRELATION: usize = 4 * 1024 * 1024;
const MAXIMUM_TOTAL_CORRELATION_BYTES: usize = 64 * 1024 * 1024;
const MAXIMUM_RAW_EVIDENCE_BYTES_PER_CORRELATION: usize = 4 * 1024 * 1024;
const MAXIMUM_TOTAL_RAW_EVIDENCE_BYTES: usize = 64 * 1024 * 1024;
const MAXIMUM_RAW_EVIDENCE_BYTES_PER_CAPABILITY: usize = MAXIMUM_TOTAL_RAW_EVIDENCE_BYTES / 2;
// Preserve room for diagnostics even when a correlation approaches its raw-series limit.
const RAW_EVIDENCE_CONFLICT_RESERVE_BYTES: usize = 256 * 1024;
const MAXIMUM_RAW_SERIES_BYTES_PER_CORRELATION: usize =
    MAXIMUM_RAW_EVIDENCE_BYTES_PER_CORRELATION - RAW_EVIDENCE_CONFLICT_RESERVE_BYTES;
const MAXIMUM_OBSERVATION_CONFLICTS: usize = 64;
const MAXIMUM_OBSERVATION_CONFLICT_VALUE_BYTES: usize = 8 * 1024;
const MAXIMUM_OBSERVATION_FAILURE_IDENTITY_CHARS: usize = 256;
const MAXIMUM_OBSERVATION_FAILURE_MESSAGE_CHARS: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelationEvent {
    pub id: u64,
    pub correlation_id: String,
    #[serde(flatten)]
    pub kind: CorrelationEventKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum CorrelationEventKind {
    Command {
        operation_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        causation_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duplicate: Option<bool>,
        command: String,
        schema_version: u32,
        aggregate_type: String,
        aggregate_id: String,
    },
    DomainEvent {
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        causation_id: Option<String>,
        event_type: String,
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        aggregate_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        aggregate_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stream_version: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    IntegrationEvent {
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        causation_id: Option<String>,
        event_type: String,
        schema_version: u32,
        subject: String,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainEventObservation {
    pub message_id: String,
    pub causation_id: Option<String>,
    pub event_type: String,
    pub schema_version: u32,
    pub aggregate_type: Option<String>,
    pub aggregate_id: Option<String>,
    pub stream_version: Option<u64>,
    pub payload: Option<Value>,
}

impl DomainEventObservation {
    pub fn new(
        message_id: impl Into<String>,
        event_type: impl Into<String>,
        schema_version: u32,
    ) -> Self {
        Self {
            message_id: message_id.into(),
            causation_id: None,
            event_type: event_type.into(),
            schema_version,
            aggregate_type: None,
            aggregate_id: None,
            stream_version: None,
            payload: None,
        }
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: impl Into<String>) -> Self {
        self.causation_id = Some(causation_id.into());
        self
    }

    #[must_use]
    pub fn with_aggregate(
        mut self,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
    ) -> Self {
        self.aggregate_type = Some(aggregate_type.into());
        self.aggregate_id = Some(aggregate_id.into());
        self
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationEventObservation {
    pub message_id: String,
    pub causation_id: Option<String>,
    pub event_type: String,
    pub schema_version: u32,
    pub subject: String,
    pub payload: Option<Value>,
}

impl IntegrationEventObservation {
    pub fn new(
        message_id: impl Into<String>,
        event_type: impl Into<String>,
        schema_version: u32,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            message_id: message_id.into(),
            causation_id: None,
            event_type: event_type.into(),
            schema_version,
            subject: subject.into(),
            payload: None,
        }
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: impl Into<String>) -> Self {
        self.causation_id = Some(causation_id.into());
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelationObservationConflict {
    pub identity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrelationObservationFailure {
    pub identity: String,
    pub message: String,
    pub count: u64,
}

#[derive(Clone)]
pub struct CorrelationEvidenceSnapshot {
    pub observed: ObservedMessageSeries,
    pub conflicts: Vec<CorrelationObservationConflict>,
    pub failure: Option<CorrelationObservationFailure>,
    pub revision: u64,
}

#[derive(Clone)]
pub struct CorrelationObserver {
    hub: Arc<CorrelationHub>,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
    mode: OperationMode,
}

impl CorrelationObserver {
    pub async fn record_observation_failure(
        &self,
        correlation_id: &str,
        identity: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), CorrelationError> {
        let identity = identity.into();
        let message = message.into();
        self.hub
            .record_observation_failure_for_mode(
                correlation_id,
                self.mode,
                CorrelationObservationFailure {
                    identity: bounded_observation_text(
                        &identity,
                        MAXIMUM_OBSERVATION_FAILURE_IDENTITY_CHARS,
                    ),
                    message: bounded_observation_text(
                        &message,
                        MAXIMUM_OBSERVATION_FAILURE_MESSAGE_CHARS,
                    ),
                    count: 1,
                },
            )
            .await
    }

    pub async fn observe_domain_event(
        &self,
        correlation_id: &str,
        observation: DomainEventObservation,
    ) -> Result<(), CorrelationError> {
        let aggregate = domain_event_aggregate(&observation)?;
        let raw = ObservedMessageNode::domain_event(
            observation.message_id.clone(),
            correlation_id,
            observation.causation_id.clone(),
            observation.event_type.clone(),
            observation.schema_version,
            aggregate,
            observation.payload.clone(),
        );
        if self
            .hub
            .observe_message_for_mode(correlation_id, self.mode, raw)
            .await?
            .is_duplicate()
        {
            return Ok(());
        }
        let payload = observation
            .payload
            .and_then(|payload| self.trace_payload_policy.observed_event_payload(payload));
        self.hub
            .observe_for_mode(
                correlation_id,
                self.mode,
                CorrelationEventKind::DomainEvent {
                    message_id: observation.message_id,
                    causation_id: observation.causation_id,
                    event_type: observation.event_type,
                    schema_version: observation.schema_version,
                    aggregate_type: observation.aggregate_type,
                    aggregate_id: observation.aggregate_id,
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
        let raw = ObservedMessageNode::integration_event(
            observation.message_id.clone(),
            correlation_id,
            observation.causation_id.clone(),
            observation.event_type.clone(),
            observation.schema_version,
            observation.payload.clone(),
        );
        if self
            .hub
            .observe_message_for_mode(correlation_id, self.mode, raw)
            .await?
            .is_duplicate()
        {
            return Ok(());
        }
        let payload = observation
            .payload
            .and_then(|payload| self.trace_payload_policy.observed_event_payload(payload));
        self.hub
            .observe_for_mode(
                correlation_id,
                self.mode,
                CorrelationEventKind::IntegrationEvent {
                    message_id: observation.message_id,
                    causation_id: observation.causation_id,
                    event_type: observation.event_type,
                    schema_version: observation.schema_version,
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

pub struct CorrelationEvidenceSubscription {
    record: Arc<CorrelationRecord>,
    receiver: watch::Receiver<u64>,
    revision: u64,
}

impl Drop for CorrelationEvidenceSubscription {
    fn drop(&mut self) {
        self.record.subscribers.fetch_sub(1, Ordering::AcqRel);
    }
}

impl CorrelationEvidenceSubscription {
    pub(super) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) async fn snapshot(
        &mut self,
    ) -> Result<CorrelationEvidenceSnapshot, CorrelationError> {
        let snapshot = self.record.evidence_snapshot().await?;
        self.revision = snapshot.revision;
        Ok(snapshot)
    }

    pub(super) async fn changed(&mut self) -> Result<(), CorrelationError> {
        loop {
            let revision = self.record.evidence_revision()?;
            if revision != self.revision {
                self.revision = revision;
                return Ok(());
            }
            self.receiver
                .changed()
                .await
                .map_err(|_| CorrelationError::NotFound)?;
        }
    }
}

impl Drop for CorrelationSubscription {
    fn drop(&mut self) {
        self.record.subscribers.fetch_sub(1, Ordering::AcqRel);
    }
}

impl CorrelationSubscription {
    pub async fn next(&mut self) -> Option<CorrelationEvent> {
        loop {
            let (event, is_closed, is_lagged) = {
                let state = self.record.state.lock().await;
                state.event_after_or_closed(self.cursor, &self.record.closed)
            };
            if is_lagged {
                return None;
            }
            if let Some(event) = event {
                self.cursor = event.id;
                return Some(event);
            }
            if is_closed {
                return None;
            }
            if self.receiver.changed().await.is_err() {
                return None;
            }
        }
    }
}

pub struct CorrelationHub {
    state: StdMutex<CorrelationTable>,
    maximum_correlations: usize,
    maximum_events_per_correlation: usize,
    maximum_bytes_per_correlation: usize,
    control_raw_evidence_budget: Arc<RawEvidenceBudget>,
    dispatch_raw_evidence_budget: Arc<RawEvidenceBudget>,
}

impl CorrelationHub {
    pub fn new(maximum_correlations: usize) -> Arc<Self> {
        let maximum_correlations = maximum_correlations.max(1);
        Arc::new(Self {
            state: StdMutex::new(CorrelationTable::default()),
            maximum_correlations,
            maximum_events_per_correlation: DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION,
            maximum_bytes_per_correlation: correlation_byte_budget(maximum_correlations),
            control_raw_evidence_budget: Arc::new(RawEvidenceBudget::new(
                MAXIMUM_RAW_EVIDENCE_BYTES_PER_CAPABILITY,
            )),
            dispatch_raw_evidence_budget: Arc::new(RawEvidenceBudget::new(
                MAXIMUM_RAW_EVIDENCE_BYTES_PER_CAPABILITY,
            )),
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
        let mut table = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if table.records.contains_key(correlation_id) {
            return Err(CorrelationError::InvalidId(
                "correlation is already registered".to_owned(),
            ));
        }
        if table.records.len() >= self.maximum_correlations {
            return Err(CorrelationError::CapacityExhausted);
        }
        let raw_evidence_budget = match mode {
            OperationMode::Simulate | OperationMode::Test => {
                Arc::clone(&self.control_raw_evidence_budget)
            }
            OperationMode::Dispatch => Arc::clone(&self.dispatch_raw_evidence_budget),
        };
        let record = CorrelationRecord::new(
            correlation_id.to_owned(),
            mode,
            self.maximum_events_per_correlation,
            self.maximum_bytes_per_correlation,
            raw_evidence_budget,
            CorrelationEventKind::Command {
                operation_id,
                message_id: None,
                causation_id: None,
                duplicate: None,
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn observe_command(
        &self,
        correlation_id: &str,
        operation_id: String,
        message_id: String,
        causation_id: Option<String>,
        duplicate: bool,
        command: String,
        schema_version: u32,
        aggregate: TestAggregate,
        payload: Option<Value>,
    ) -> Result<(), CorrelationError> {
        let aggregate_type = aggregate.aggregate_type.clone();
        let aggregate_id = aggregate.id.clone();
        let message = ObservedMessageNode::command(
            message_id.clone(),
            correlation_id,
            causation_id.clone(),
            command.clone(),
            schema_version,
            aggregate,
            payload,
        );
        if self
            .observe_message(correlation_id, message)
            .await?
            .is_duplicate()
        {
            return Ok(());
        }
        self.observe(
            correlation_id,
            CorrelationEventKind::Command {
                operation_id,
                message_id: Some(message_id),
                causation_id,
                duplicate: Some(duplicate),
                command,
                schema_version,
                aggregate_type,
                aggregate_id,
            },
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn observe_simulated_command(
        &self,
        correlation_id: &str,
        message_id: String,
        command: String,
        schema_version: u32,
        aggregate: TestAggregate,
        payload: Option<Value>,
    ) -> Result<(), CorrelationError> {
        let message = ObservedMessageNode::command(
            message_id,
            correlation_id,
            None,
            command,
            schema_version,
            aggregate,
            payload,
        );
        self.observe_message(correlation_id, message)
            .await
            .map(|_| ())
    }

    pub(crate) async fn observe_command_outcome(
        &self,
        correlation_id: &str,
        response_message_id: String,
        command_message_id: String,
        outcome: CommandResponseOutcome,
    ) -> Result<(), CorrelationError> {
        let outcome = ObservedCommandOutcome::try_new(
            response_message_id,
            command_message_id,
            correlation_id,
            outcome,
        )
        .map_err(|error| observation_error(&error))?;
        validate_correlation_id(correlation_id)?;
        let record = self.record(correlation_id)?;
        record.insert_command_outcome(outcome).await.map(|_| ())
    }

    #[cfg(test)]
    async fn observed_message_series(
        &self,
        correlation_id: &str,
    ) -> Result<ObservedMessageSeries, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        self.record(correlation_id)?.observed_message_series().await
    }

    #[allow(
        dead_code,
        reason = "the service diagnostics integration consumes this snapshot API"
    )]
    pub(crate) async fn observation_conflicts(
        &self,
        correlation_id: &str,
    ) -> Result<Vec<CorrelationObservationConflict>, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        self.record(correlation_id)?.observation_conflicts().await
    }

    pub(crate) async fn evidence_snapshot(
        &self,
        correlation_id: &str,
    ) -> Result<CorrelationEvidenceSnapshot, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        self.record(correlation_id)?.evidence_snapshot().await
    }

    pub(super) fn evidence_revision(&self, correlation_id: &str) -> Result<u64, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        self.record(correlation_id)?.evidence_revision()
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

    async fn observe_message(
        &self,
        correlation_id: &str,
        message: ObservedMessageNode,
    ) -> Result<MessageSeriesInsertOutcome, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        let record = self.record(correlation_id)?;
        record.insert_message(message).await
    }

    async fn observe_message_for_mode(
        &self,
        correlation_id: &str,
        mode: OperationMode,
        message: ObservedMessageNode,
    ) -> Result<MessageSeriesInsertOutcome, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        let record = self.record(correlation_id)?;
        if record.mode != mode {
            return Err(CorrelationError::InvalidId(
                "correlation does not belong to the observer environment".to_owned(),
            ));
        }
        record.insert_message(message).await
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

    async fn record_observation_failure_for_mode(
        &self,
        correlation_id: &str,
        mode: OperationMode,
        failure: CorrelationObservationFailure,
    ) -> Result<(), CorrelationError> {
        validate_correlation_id(correlation_id)?;
        let record = self.record(correlation_id)?;
        if record.mode != mode {
            return Err(CorrelationError::InvalidId(
                "correlation does not belong to the observer environment".to_owned(),
            ));
        }
        record.retain_observation_failure(failure).await
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
        let lifecycle = record
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if record.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let latest = state.next_id.saturating_sub(1);
        let oldest = state.events.front().map_or(state.next_id, |event| event.id);
        if after > latest {
            return Err(CorrelationError::FutureCursor { latest });
        }
        if after.saturating_add(1) < oldest {
            return Err(CorrelationError::ExpiredCursor { oldest });
        }
        record.subscribers.fetch_add(1, Ordering::AcqRel);
        drop(lifecycle);
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

    pub(super) async fn subscribe_evidence(
        &self,
        correlation_id: &str,
    ) -> Result<CorrelationEvidenceSubscription, CorrelationError> {
        let record = self.record(correlation_id)?;
        let state = record.state.lock().await;
        let lifecycle = record
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if record.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let revision = record
            .evidence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revision;
        record.subscribers.fetch_add(1, Ordering::AcqRel);
        let receiver = record.evidence_changed.subscribe();
        drop(lifecycle);
        drop(state);
        Ok(CorrelationEvidenceSubscription {
            record,
            receiver,
            revision,
        })
    }

    pub fn retain_dispatch_correlations(&self) {
        let mut table = self.state.lock().unwrap_or_else(PoisonError::into_inner);
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

    pub fn has_active_subscribers(&self, correlation_id: &str) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .records
            .get(correlation_id)
            .is_some_and(|record| record.subscribers.load(Ordering::Acquire) > 0)
    }

    pub fn remove_if_inactive(&self, correlation_id: &str) -> bool {
        let mut table = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(record) = table.records.get(correlation_id).cloned() else {
            return true;
        };
        let lifecycle = record
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if record.subscribers.load(Ordering::Acquire) > 0 {
            return false;
        }
        table
            .insertion_order
            .retain(|retained| retained != correlation_id);
        table.records.remove(correlation_id);
        drop(table);
        record.close_locked();
        drop(lifecycle);
        true
    }

    fn record(&self, correlation_id: &str) -> Result<Arc<CorrelationRecord>, CorrelationError> {
        validate_correlation_id(correlation_id)?;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            control_raw_evidence_budget: Arc::new(RawEvidenceBudget::new(
                MAXIMUM_RAW_EVIDENCE_BYTES_PER_CAPABILITY,
            )),
            dispatch_raw_evidence_budget: Arc::new(RawEvidenceBudget::new(
                MAXIMUM_RAW_EVIDENCE_BYTES_PER_CAPABILITY,
            )),
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
    evidence: StdMutex<CorrelationEvidenceState>,
    evidence_changed: watch::Sender<u64>,
    raw_evidence_budget: Arc<RawEvidenceBudget>,
    lifecycle: StdMutex<()>,
    changed: watch::Sender<u64>,
    closed: AtomicBool,
    subscribers: AtomicUsize,
}

impl CorrelationRecord {
    fn new(
        correlation_id: String,
        mode: OperationMode,
        maximum_events: usize,
        maximum_bytes: usize,
        raw_evidence_budget: Arc<RawEvidenceBudget>,
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
        let (evidence_changed, _) = watch::channel(0);
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
            evidence: StdMutex::new(CorrelationEvidenceState::default()),
            evidence_changed,
            raw_evidence_budget,
            lifecycle: StdMutex::new(()),
            changed,
            closed: AtomicBool::new(false),
            subscribers: AtomicUsize::new(0),
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
            .unwrap_or_else(PoisonError::into_inner);
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
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

    async fn insert_message(
        &self,
        message: ObservedMessageNode,
    ) -> Result<MessageSeriesInsertOutcome, CorrelationError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let (result, notify) = {
            let _state = self.state.lock().await;
            let _lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if self.closed.load(Ordering::Acquire) {
                return Err(CorrelationError::NotFound);
            }
            let mut evidence = self.evidence.lock().unwrap_or_else(PoisonError::into_inner);
            self.insert_message_evidence(&mut evidence, &message)
        };
        if notify {
            self.notify_evidence_change();
        }
        result
    }

    async fn insert_command_outcome(
        &self,
        outcome: ObservedCommandOutcome,
    ) -> Result<MessageSeriesInsertOutcome, CorrelationError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let (result, notify) = {
            let _state = self.state.lock().await;
            let _lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if self.closed.load(Ordering::Acquire) {
                return Err(CorrelationError::NotFound);
            }
            let mut evidence = self.evidence.lock().unwrap_or_else(PoisonError::into_inner);
            self.insert_command_outcome_evidence(&mut evidence, &outcome)
        };
        if notify {
            self.notify_evidence_change();
        }
        result
    }

    #[cfg(test)]
    async fn observed_message_series(&self) -> Result<ObservedMessageSeries, CorrelationError> {
        self.evidence_snapshot()
            .await
            .map(|snapshot| snapshot.observed)
    }

    #[allow(
        dead_code,
        reason = "the hub exposes this through the pending service diagnostics integration"
    )]
    async fn observation_conflicts(
        &self,
    ) -> Result<Vec<CorrelationObservationConflict>, CorrelationError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let _state = self.state.lock().await;
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        Ok(self
            .evidence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .conflicts
            .iter()
            .cloned()
            .collect())
    }

    async fn evidence_snapshot(&self) -> Result<CorrelationEvidenceSnapshot, CorrelationError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let _state = self.state.lock().await;
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let evidence = self.evidence.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(CorrelationEvidenceSnapshot {
            observed: evidence.observed.clone(),
            conflicts: evidence.conflicts.iter().cloned().collect(),
            failure: evidence.failure.clone(),
            revision: evidence.revision,
        })
    }

    fn evidence_revision(&self) -> Result<u64, CorrelationError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        Ok(self
            .evidence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revision)
    }

    async fn retain_observation_failure(
        &self,
        failure: CorrelationObservationFailure,
    ) -> Result<(), CorrelationError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let _state = self.state.lock().await;
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.closed.load(Ordering::Acquire) {
            return Err(CorrelationError::NotFound);
        }
        let mut evidence = self.evidence.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(retained) = &mut evidence.failure {
            retained.count = retained.count.saturating_add(1);
        } else {
            evidence.failure = Some(failure);
        }
        evidence.revision = evidence.revision.saturating_add(1);
        drop(evidence);
        self.notify_evidence_change();
        Ok(())
    }

    fn insert_message_evidence(
        &self,
        evidence: &mut CorrelationEvidenceState,
        message: &ObservedMessageNode,
    ) -> (Result<MessageSeriesInsertOutcome, CorrelationError>, bool) {
        let existing = evidence
            .observed
            .messages()
            .get(message.message_id())
            .cloned();
        let mut observed = evidence.observed.clone();
        match observed.insert_message(message.clone()) {
            Ok(MessageSeriesInsertOutcome::Duplicate) => {
                (Ok(MessageSeriesInsertOutcome::Duplicate), false)
            }
            Ok(MessageSeriesInsertOutcome::Inserted) => {
                let result = self.replace_observed(evidence, observed);
                let notify = result.is_ok();
                (
                    result.map(|()| MessageSeriesInsertOutcome::Inserted),
                    notify,
                )
            }
            Err(error) => {
                let original = observation_error(&error);
                let Some(existing) = existing else {
                    return (Err(original), false);
                };
                let conflict =
                    observation_conflict(message.message_id(), &error, &existing, message);
                match self.retain_conflict(evidence, conflict) {
                    Ok(()) => (Err(original), true),
                    Err(capacity) => (Err(capacity), false),
                }
            }
        }
    }

    fn insert_command_outcome_evidence(
        &self,
        evidence: &mut CorrelationEvidenceState,
        outcome: &ObservedCommandOutcome,
    ) -> (Result<MessageSeriesInsertOutcome, CorrelationError>, bool) {
        let existing = evidence
            .observed
            .command_outcomes()
            .iter()
            .find(|existing| {
                existing.command_message_id() == outcome.command_message_id()
                    || existing.response_message_id() == outcome.response_message_id()
            })
            .cloned();
        let mut observed = evidence.observed.clone();
        match observed.insert_command_outcome(outcome.clone()) {
            Ok(MessageSeriesInsertOutcome::Duplicate) => {
                (Ok(MessageSeriesInsertOutcome::Duplicate), false)
            }
            Ok(MessageSeriesInsertOutcome::Inserted) => {
                let result = self.replace_observed(evidence, observed);
                let notify = result.is_ok();
                (
                    result.map(|()| MessageSeriesInsertOutcome::Inserted),
                    notify,
                )
            }
            Err(error) => {
                let original = observation_error(&error);
                let Some(existing) = existing else {
                    return (Err(original), false);
                };
                let identity = if existing.command_message_id() == outcome.command_message_id() {
                    outcome.command_message_id()
                } else {
                    outcome.response_message_id()
                };
                let conflict = observation_conflict(identity, &error, &existing, outcome);
                match self.retain_conflict(evidence, conflict) {
                    Ok(()) => (Err(original), true),
                    Err(capacity) => (Err(capacity), false),
                }
            }
        }
    }

    fn replace_observed(
        &self,
        evidence: &mut CorrelationEvidenceState,
        observed: ObservedMessageSeries,
    ) -> Result<(), CorrelationError> {
        if serialized_len(&observed)? > MAXIMUM_RAW_SERIES_BYTES_PER_CORRELATION {
            return Err(CorrelationError::EventTooLarge);
        }
        let mut conflicts = evidence.conflicts.clone();
        let retained_bytes = fit_raw_evidence(&observed, &mut conflicts, 0)?;
        self.replace_evidence(evidence, observed, conflicts, retained_bytes)
    }

    fn retain_conflict(
        &self,
        evidence: &mut CorrelationEvidenceState,
        conflict: CorrelationObservationConflict,
    ) -> Result<(), CorrelationError> {
        let mut conflicts = evidence.conflicts.clone();
        conflicts.push_back(conflict);
        while conflicts.len() > MAXIMUM_OBSERVATION_CONFLICTS {
            conflicts.pop_front();
        }
        let retained_bytes = fit_raw_evidence(&evidence.observed, &mut conflicts, 1)?;
        let observed = evidence.observed.clone();
        self.replace_evidence(evidence, observed, conflicts, retained_bytes)
    }

    fn replace_evidence(
        &self,
        evidence: &mut CorrelationEvidenceState,
        observed: ObservedMessageSeries,
        conflicts: VecDeque<CorrelationObservationConflict>,
        retained_bytes: usize,
    ) -> Result<(), CorrelationError> {
        self.raw_evidence_budget
            .replace(evidence.retained_bytes, retained_bytes)?;
        evidence.observed = observed;
        evidence.conflicts = conflicts;
        evidence.retained_bytes = retained_bytes;
        evidence.revision = evidence.revision.saturating_add(1);
        Ok(())
    }

    fn close(&self) {
        let _lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.close_locked();
    }

    fn close_locked(&self) {
        self.closed.store(true, Ordering::Release);
        self.clear_evidence();
        self.evidence_changed.send_replace(0);
        self.notify_state_change();
    }

    fn clear_evidence(&self) {
        let released = {
            let mut evidence = self.evidence.lock().unwrap_or_else(PoisonError::into_inner);
            let released = evidence.retained_bytes;
            *evidence = CorrelationEvidenceState::default();
            released
        };
        self.raw_evidence_budget.release(released);
    }

    fn notify_state_change(&self) {
        let latest = *self.changed.borrow();
        self.changed.send_replace(latest);
    }

    fn notify_evidence_change(&self) {
        let revision = self
            .evidence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revision;
        self.evidence_changed.send_replace(revision);
        self.notify_state_change();
    }
}

impl Drop for CorrelationRecord {
    fn drop(&mut self) {
        self.clear_evidence();
    }
}

struct CorrelationState {
    next_id: u64,
    events: VecDeque<CorrelationEvent>,
    retained_bytes: usize,
}

#[derive(Default)]
struct CorrelationEvidenceState {
    observed: ObservedMessageSeries,
    conflicts: VecDeque<CorrelationObservationConflict>,
    failure: Option<CorrelationObservationFailure>,
    retained_bytes: usize,
    revision: u64,
}

struct RawEvidenceBudget {
    maximum_bytes: usize,
    retained_bytes: StdMutex<usize>,
}

impl RawEvidenceBudget {
    const fn new(maximum_bytes: usize) -> Self {
        Self {
            maximum_bytes,
            retained_bytes: StdMutex::new(0),
        }
    }

    fn replace(&self, current: usize, replacement: usize) -> Result<(), CorrelationError> {
        let mut retained = self
            .retained_bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let without_current = retained
            .checked_sub(current)
            .ok_or(CorrelationError::CapacityExhausted)?;
        let next = without_current
            .checked_add(replacement)
            .filter(|next| *next <= self.maximum_bytes)
            .ok_or(CorrelationError::CapacityExhausted)?;
        *retained = next;
        drop(retained);
        Ok(())
    }

    fn release(&self, released: usize) {
        let mut retained = self
            .retained_bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *retained = retained.saturating_sub(released);
        drop(retained);
    }

    #[cfg(test)]
    fn retained_bytes(&self) -> usize {
        *self
            .retained_bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedRawEvidence<'a> {
    observed_message_series: &'a ObservedMessageSeries,
    conflicts: &'a VecDeque<CorrelationObservationConflict>,
}

impl CorrelationState {
    fn event_after_or_closed(
        &self,
        cursor: u64,
        closed: &AtomicBool,
    ) -> (Option<CorrelationEvent>, bool, bool) {
        let is_lagged = self
            .events
            .front()
            .is_some_and(|event| cursor.saturating_add(1) < event.id);
        let event = self.events.iter().find(|event| event.id > cursor).cloned();
        let is_closed = event.is_none() && closed.load(Ordering::Acquire);
        (event, is_closed, is_lagged)
    }
}

fn serialized_event_len(event: &CorrelationEvent) -> usize {
    serde_json::to_vec(event).map_or(usize::MAX, |serialized| serialized.len())
}

fn serialized_len<T>(value: &T) -> Result<usize, CorrelationError>
where
    T: Serialize + ?Sized,
{
    serde_json::to_vec(value)
        .map(|serialized| serialized.len())
        .map_err(|_| CorrelationError::EventTooLarge)
}

fn fit_raw_evidence(
    observed: &ObservedMessageSeries,
    conflicts: &mut VecDeque<CorrelationObservationConflict>,
    minimum_conflicts: usize,
) -> Result<usize, CorrelationError> {
    loop {
        let retained_bytes = serialized_len(&SerializedRawEvidence {
            observed_message_series: observed,
            conflicts,
        })?;
        if retained_bytes <= MAXIMUM_RAW_EVIDENCE_BYTES_PER_CORRELATION {
            return Ok(retained_bytes);
        }
        if conflicts.len() <= minimum_conflicts {
            return Err(CorrelationError::EventTooLarge);
        }
        conflicts.pop_front();
    }
}

fn observation_conflict<Existing, Observed>(
    identity: &str,
    error: &ObservedMessageSeriesError,
    existing: &Existing,
    observed: &Observed,
) -> CorrelationObservationConflict
where
    Existing: Serialize + ?Sized,
    Observed: Serialize + ?Sized,
{
    CorrelationObservationConflict {
        identity: identity.to_owned(),
        message: error.to_string(),
        existing: bounded_conflict_value(existing),
        observed: bounded_conflict_value(observed),
    }
}

fn bounded_conflict_value<T>(value: &T) -> Option<Value>
where
    T: Serialize + ?Sized,
{
    let value = serde_json::to_value(value).ok()?;
    (serialized_len(&value).ok()? <= MAXIMUM_OBSERVATION_CONFLICT_VALUE_BYTES).then_some(value)
}

fn observation_error(error: &ObservedMessageSeriesError) -> CorrelationError {
    CorrelationError::InvalidId(error.to_string())
}

fn bounded_observation_text(value: &str, maximum_chars: usize) -> String {
    value.chars().take(maximum_chars).collect()
}

fn domain_event_aggregate(
    observation: &DomainEventObservation,
) -> Result<Option<TestAggregate>, CorrelationError> {
    match (&observation.aggregate_type, &observation.aggregate_id) {
        (Some(aggregate_type), Some(aggregate_id)) => Ok(Some(TestAggregate {
            aggregate_type: aggregate_type.clone(),
            id: aggregate_id.clone(),
        })),
        (None, None) => Ok(None),
        _ => Err(CorrelationError::InvalidId(
            "domain event aggregate type and ID must both be present or both be absent".to_owned(),
        )),
    }
}

fn correlation_byte_budget(maximum_correlations: usize) -> usize {
    MAXIMUM_TOTAL_CORRELATION_BYTES
        .checked_div(maximum_correlations.max(1))
        .unwrap_or(MAXIMUM_TOTAL_CORRELATION_BYTES)
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

pub fn validate_correlation_id(value: &str) -> Result<(), CorrelationError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(CorrelationError::InvalidId(
            "correlation ID must contain 1-256 non-control characters".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "test fixture construction and asynchronous setup must succeed"
    )]

    use std::time::Duration;

    use rostfrei_messaging_core::CommandResponseOutcome;
    use serde_json::json;

    use super::*;

    fn test_hub(
        maximum_correlations: usize,
        maximum_events: usize,
        maximum_raw_bytes: usize,
    ) -> Arc<CorrelationHub> {
        Arc::new(CorrelationHub {
            state: StdMutex::new(CorrelationTable::default()),
            maximum_correlations,
            maximum_events_per_correlation: maximum_events,
            maximum_bytes_per_correlation: DEFAULT_MAXIMUM_BYTES_PER_CORRELATION,
            control_raw_evidence_budget: Arc::new(RawEvidenceBudget::new(maximum_raw_bytes)),
            dispatch_raw_evidence_budget: Arc::new(RawEvidenceBudget::new(maximum_raw_bytes)),
        })
    }

    fn register(hub: &CorrelationHub, correlation_id: &str) {
        register_mode(hub, correlation_id, OperationMode::Test);
    }

    fn register_mode(hub: &CorrelationHub, correlation_id: &str, mode: OperationMode) {
        hub.register_command(
            correlation_id,
            mode,
            format!("operation-{correlation_id}"),
            "rent-bicycle".to_owned(),
            1,
            "bike-rental/rental-fleet".to_owned(),
            "fleet-1".to_owned(),
        )
        .expect("register correlation");
    }

    fn raw_domain_message(correlation_id: &str, message_id: &str) -> ObservedMessageNode {
        ObservedMessageNode::domain_event(
            message_id,
            correlation_id,
            None,
            "bicycle-rented",
            1,
            None,
            Some(json!({ "bicycleId": "bike-1" })),
        )
    }

    fn retained_raw_bytes(hub: &CorrelationHub, correlation_id: &str) -> usize {
        let record = hub.record(correlation_id).expect("correlation record");
        record
            .evidence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retained_bytes
    }

    #[tokio::test]
    async fn observations_retain_raw_payload_and_publish_redacted_exact_identity() {
        let hub = CorrelationHub::new(1);
        register(&hub, "correlation-1");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));
        let domain = DomainEventObservation::new("event-1", "bicycle-rented", 2)
            .with_causation_id("command-1")
            .with_aggregate("bike-rental/rental-fleet", "fleet-1")
            .with_stream_version(7)
            .with_payload(json!({ "bicycleId": "bike-1", "secret": true }));
        observer
            .observe_domain_event("correlation-1", domain.clone())
            .await
            .expect("observe domain event");
        observer
            .observe_domain_event("correlation-1", domain)
            .await
            .expect("observe duplicate domain event");
        let integration = IntegrationEventObservation::new(
            "integration-1",
            "bicycle-rental-started",
            3,
            "bike-test.integration.bike-rental.bicycle-rental-started",
        )
        .with_causation_id("event-1")
        .with_payload(json!({ "fleetId": "fleet-1", "private": true }));
        observer
            .observe_integration_event("correlation-1", integration.clone())
            .await
            .expect("observe integration event");
        observer
            .observe_integration_event("correlation-1", integration)
            .await
            .expect("observe duplicate integration event");

        let raw = hub
            .observed_message_series("correlation-1")
            .await
            .expect("raw message series");
        assert_eq!(raw.messages().len(), 2);
        let domain = raw.messages().get("event-1").expect("raw domain event");
        assert_eq!(domain.correlation_id(), "correlation-1");
        assert_eq!(domain.causation_id(), Some("command-1"));
        assert_eq!(domain.name(), "bicycle-rented");
        assert_eq!(domain.schema_version(), 2);
        assert_eq!(
            domain.aggregate(),
            Some(&TestAggregate {
                aggregate_type: "bike-rental/rental-fleet".to_owned(),
                id: "fleet-1".to_owned(),
            })
        );
        assert_eq!(
            domain.payload(),
            Some(&json!({ "bicycleId": "bike-1", "secret": true }))
        );
        let integration = raw
            .messages()
            .get("integration-1")
            .expect("raw integration event");
        assert_eq!(integration.causation_id(), Some("event-1"));
        assert_eq!(
            integration.payload(),
            Some(&json!({ "fleetId": "fleet-1", "private": true }))
        );

        let mut subscription = hub
            .subscribe("correlation-1", 1)
            .await
            .expect("correlation subscription");
        let domain = subscription.next().await.expect("public domain event");
        assert!(matches!(
            domain.kind,
            CorrelationEventKind::DomainEvent {
                ref message_id,
                causation_id: Some(ref causation_id),
                ref event_type,
                schema_version: 2,
                aggregate_type: Some(ref aggregate_type),
                aggregate_id: Some(ref aggregate_id),
                stream_version: Some(7),
                payload: None,
            } if message_id == "event-1"
                && causation_id == "command-1"
                && event_type == "bicycle-rented"
                && aggregate_type == "bike-rental/rental-fleet"
                && aggregate_id == "fleet-1"
        ));
        let integration = subscription.next().await.expect("public integration event");
        assert!(matches!(
            integration.kind,
            CorrelationEventKind::IntegrationEvent {
                ref message_id,
                causation_id: Some(ref causation_id),
                ref event_type,
                schema_version: 3,
                ref subject,
                payload: None,
            } if message_id == "integration-1"
                && causation_id == "event-1"
                && event_type == "bicycle-rental-started"
                && subject == "bike-test.integration.bike-rental.bicycle-rental-started"
        ));
        assert!(hub.subscribe("correlation-1", 3).await.is_ok());
    }

    #[tokio::test]
    async fn observation_failures_are_bounded_retained_and_versioned() {
        let hub = CorrelationHub::new(1);
        register(&hub, "correlation-1");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));

        observer
            .record_observation_failure("correlation-1", "x".repeat(1024), "y".repeat(4096))
            .await
            .expect("retain first observation failure");
        observer
            .record_observation_failure("correlation-1", "event-2", "second failure")
            .await
            .expect("retain repeated observation failure");

        let snapshot = hub
            .evidence_snapshot("correlation-1")
            .await
            .expect("evidence snapshot");
        let failure = snapshot.failure.expect("retained observation failure");
        assert_eq!(failure.count, 2);
        assert_eq!(
            failure.identity.chars().count(),
            MAXIMUM_OBSERVATION_FAILURE_IDENTITY_CHARS
        );
        assert_eq!(
            failure.message.chars().count(),
            MAXIMUM_OBSERVATION_FAILURE_MESSAGE_CHARS
        );
        assert_eq!(snapshot.revision, 2);
    }

    #[tokio::test]
    async fn incomplete_domain_aggregate_identity_is_rejected_before_observation() {
        let hub = CorrelationHub::new(1);
        register(&hub, "correlation-1");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));
        let incomplete = [
            (Some("bike-rental/rental-fleet".to_owned()), None),
            (None, Some("fleet-1".to_owned())),
        ];
        for (index, (aggregate_type, aggregate_id)) in incomplete.into_iter().enumerate() {
            let mut observation =
                DomainEventObservation::new(format!("event-{index}"), "bicycle-rented", 1);
            observation.aggregate_type = aggregate_type;
            observation.aggregate_id = aggregate_id;
            let error = observer
                .observe_domain_event("correlation-1", observation)
                .await
                .expect_err("partial aggregate identity must be rejected");
            assert!(matches!(
                error,
                CorrelationError::InvalidId(ref message)
                    if message.contains("aggregate type and ID must both be present")
            ));
        }

        assert!(
            hub.observed_message_series("correlation-1")
                .await
                .expect("raw message series")
                .messages()
                .is_empty()
        );
        assert!(hub.subscribe("correlation-1", 1).await.is_ok());
    }

    #[tokio::test]
    async fn exact_duplicate_does_not_grow_or_notify_raw_evidence() {
        let hub = CorrelationHub::new(1);
        register(&hub, "correlation-1");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));
        let observation = DomainEventObservation::new("event-1", "bicycle-rented", 1)
            .with_causation_id("command-1")
            .with_payload(json!({ "bicycleId": "bike-1" }));
        observer
            .observe_domain_event("correlation-1", observation.clone())
            .await
            .expect("first observation");
        let retained = retained_raw_bytes(&hub, "correlation-1");
        let mut subscription = hub
            .subscribe("correlation-1", 2)
            .await
            .expect("correlation subscription");

        observer
            .observe_domain_event("correlation-1", observation)
            .await
            .expect("exact duplicate");

        assert_eq!(retained_raw_bytes(&hub, "correlation-1"), retained);
        assert!(
            hub.observation_conflicts("correlation-1")
                .await
                .expect("conflict snapshot")
                .is_empty()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(20), subscription.receiver.changed())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn conflicting_observation_identity_is_retained_and_notifies_watcher() {
        let hub = CorrelationHub::new(1);
        register(&hub, "correlation-1");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));
        observer
            .observe_domain_event(
                "correlation-1",
                DomainEventObservation::new("event-1", "bicycle-rented", 1)
                    .with_causation_id("command-1")
                    .with_payload(json!({ "bicycleId": "bike-1" })),
            )
            .await
            .expect("first observation");
        let retained = retained_raw_bytes(&hub, "correlation-1");
        let mut subscription = hub
            .subscribe("correlation-1", 2)
            .await
            .expect("correlation subscription");

        let error = observer
            .observe_domain_event(
                "correlation-1",
                DomainEventObservation::new("event-1", "bicycle-rented", 1)
                    .with_causation_id("different-command")
                    .with_payload(json!({ "bicycleId": "bike-1" })),
            )
            .await
            .expect_err("reused message identity must conflict");
        assert!(matches!(error, CorrelationError::InvalidId(_)));
        let conflicts = hub
            .observation_conflicts("correlation-1")
            .await
            .expect("conflict snapshot");
        assert_eq!(conflicts.len(), 1);
        let conflict = conflicts.first().expect("retained conflict");
        assert_eq!(conflict.identity, "event-1");
        assert!(!conflict.message.is_empty());
        assert!(conflict.existing.is_some());
        assert!(conflict.observed.is_some());
        assert_ne!(conflict.existing, conflict.observed);
        assert!(retained_raw_bytes(&hub, "correlation-1") > retained);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), subscription.receiver.changed())
                .await
                .is_ok()
        );
        assert!(hub.subscribe("correlation-1", 2).await.is_ok());
    }

    #[tokio::test]
    async fn command_publications_and_outcomes_are_raw_duplicate_idempotent() {
        let hub = CorrelationHub::new(1);
        register(&hub, "correlation-1");
        let aggregate = TestAggregate {
            aggregate_type: "bike-rental/rental-fleet".to_owned(),
            id: "fleet-1".to_owned(),
        };
        for _ in 0..2 {
            hub.observe_command(
                "correlation-1",
                "operation-1".to_owned(),
                "command-1".to_owned(),
                None,
                false,
                "rent-bicycle".to_owned(),
                1,
                aggregate.clone(),
                Some(json!({ "bicycleId": "bike-1" })),
            )
            .await
            .expect("observe command");
            hub.observe_command_outcome(
                "correlation-1",
                "response-1".to_owned(),
                "command-1".to_owned(),
                CommandResponseOutcome::Accepted,
            )
            .await
            .expect("observe command outcome");
        }

        let raw = hub
            .observed_message_series("correlation-1")
            .await
            .expect("raw message series");
        assert_eq!(raw.messages().len(), 1);
        assert_eq!(raw.command_outcomes().len(), 1);
        let command = raw.messages().get("command-1").expect("raw command");
        assert_eq!(command.aggregate(), Some(&aggregate));
        assert_eq!(command.payload(), Some(&json!({ "bicycleId": "bike-1" })));

        let conflict = hub
            .observe_command_outcome(
                "correlation-1",
                "response-2".to_owned(),
                "command-1".to_owned(),
                CommandResponseOutcome::Accepted,
            )
            .await
            .expect_err("one command cannot acquire another response identity");
        assert!(matches!(
            conflict,
            CorrelationError::InvalidId(ref message)
                if message == "observed command outcome identity conflicts"
        ));
        let conflicts = hub
            .observation_conflicts("correlation-1")
            .await
            .expect("conflict snapshot");
        assert_eq!(conflicts.len(), 1);
        let conflict = conflicts.first().expect("retained conflict");
        assert_eq!(conflict.identity, "command-1");
        assert!(conflict.existing.is_some());
        assert!(conflict.observed.is_some());

        let mut subscription = hub
            .subscribe("correlation-1", 1)
            .await
            .expect("correlation subscription");
        assert!(matches!(
            subscription.next().await.expect("published command").kind,
            CorrelationEventKind::Command {
                ref operation_id,
                message_id: Some(ref message_id),
                causation_id: None,
                duplicate: Some(false),
                ref command,
                schema_version: 1,
                ref aggregate_type,
                ref aggregate_id,
            } if operation_id == "operation-1"
                && message_id == "command-1"
                && command == "rent-bicycle"
                && aggregate_type == "bike-rental/rental-fleet"
                && aggregate_id == "fleet-1"
        ));
        assert!(hub.subscribe("correlation-1", 2).await.is_ok());
    }

    #[tokio::test]
    async fn per_correlation_oversize_is_rejected_without_partial_evidence() {
        let hub = CorrelationHub::new(DEFAULT_MAXIMUM_CORRELATIONS);
        register(&hub, "correlation-1");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));
        let error = observer
            .observe_domain_event(
                "correlation-1",
                DomainEventObservation::new("event-1", "bicycle-rented", 1).with_payload(json!({
                    "value": "x".repeat(MAXIMUM_RAW_EVIDENCE_BYTES_PER_CORRELATION)
                })),
            )
            .await
            .expect_err("oversized raw evidence must be rejected");

        assert_eq!(error, CorrelationError::EventTooLarge);
        assert!(
            hub.observed_message_series("correlation-1")
                .await
                .expect("raw message series")
                .messages()
                .is_empty()
        );
        assert_eq!(hub.control_raw_evidence_budget.retained_bytes(), 0);
        assert!(hub.subscribe("correlation-1", 1).await.is_ok());
    }

    #[tokio::test]
    async fn global_raw_capacity_is_shared_across_correlations() {
        let payload = json!({ "value": "x".repeat(512) });
        let mut one = ObservedMessageSeries::new();
        one.insert_message(ObservedMessageNode::domain_event(
            "event-1",
            "correlation-1",
            None,
            "bicycle-rented",
            1,
            None,
            Some(payload.clone()),
        ))
        .expect("build raw evidence fixture");
        let maximum_raw_bytes =
            fit_raw_evidence(&one, &mut VecDeque::new(), 0).expect("measure raw evidence fixture");
        let hub = test_hub(2, DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION, maximum_raw_bytes);
        register(&hub, "correlation-1");
        register(&hub, "correlation-2");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));

        observer
            .observe_domain_event(
                "correlation-1",
                DomainEventObservation::new("event-1", "bicycle-rented", 1)
                    .with_payload(payload.clone()),
            )
            .await
            .expect("first correlation fits global budget");
        let error = observer
            .observe_domain_event(
                "correlation-2",
                DomainEventObservation::new("event-1", "bicycle-rented", 1).with_payload(payload),
            )
            .await
            .expect_err("second correlation must exhaust global budget");

        assert_eq!(error, CorrelationError::CapacityExhausted);
        assert_eq!(
            hub.control_raw_evidence_budget.retained_bytes(),
            maximum_raw_bytes
        );
        assert!(
            hub.observed_message_series("correlation-2")
                .await
                .expect("second raw message series")
                .messages()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn control_raw_capacity_cannot_exhaust_dispatch_evidence() {
        let payload = json!({ "value": "x".repeat(512) });
        let control_message = ObservedMessageNode::domain_event(
            "control-event-1",
            "control-1",
            None,
            "bicycle-rented",
            1,
            None,
            Some(payload.clone()),
        );
        let mut fixture = ObservedMessageSeries::new();
        fixture
            .insert_message(control_message.clone())
            .expect("build raw evidence fixture");
        let maximum_raw_bytes =
            fit_raw_evidence(&fixture, &mut VecDeque::new(), 0).expect("measure raw evidence");
        let hub = test_hub(3, DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION, maximum_raw_bytes);
        register(&hub, "control-1");
        register(&hub, "control-2");
        register_mode(&hub, "dispatch-1", OperationMode::Dispatch);

        hub.observe_message("control-1", control_message)
            .await
            .expect("first control observation fills its capability budget");
        let error = hub
            .observe_message(
                "control-2",
                raw_domain_message("control-2", "control-event-2"),
            )
            .await
            .expect_err("second control observation must exhaust the control budget");
        assert_eq!(error, CorrelationError::CapacityExhausted);
        hub.observe_message(
            "dispatch-1",
            raw_domain_message("dispatch-1", "dispatch-event-1"),
        )
        .await
        .expect("dispatch retains evidence from its independent budget");
        assert_eq!(
            hub.control_raw_evidence_budget.retained_bytes(),
            maximum_raw_bytes
        );
        assert!(hub.dispatch_raw_evidence_budget.retained_bytes() > 0);
    }

    #[tokio::test]
    async fn raw_accounting_is_released_on_removal_reset_and_drop() {
        let hub = test_hub(4, DEFAULT_MAXIMUM_EVENTS_PER_CORRELATION, 64 * 1024);
        let control_budget = Arc::clone(&hub.control_raw_evidence_budget);
        let dispatch_budget = Arc::clone(&hub.dispatch_raw_evidence_budget);
        register(&hub, "test-1");
        register_mode(&hub, "dispatch-1", OperationMode::Dispatch);
        hub.observe_message("test-1", raw_domain_message("test-1", "test-event-1"))
            .await
            .expect("retain test evidence");
        hub.observe_message(
            "dispatch-1",
            raw_domain_message("dispatch-1", "dispatch-event-1"),
        )
        .await
        .expect("retain dispatch evidence");
        let dispatch_only = dispatch_budget.retained_bytes();
        assert!(control_budget.retained_bytes() > 0);
        assert!(dispatch_only > 0);

        assert!(hub.remove_if_inactive("test-1"));
        assert_eq!(control_budget.retained_bytes(), 0);
        assert_eq!(dispatch_budget.retained_bytes(), dispatch_only);

        register(&hub, "test-2");
        hub.observe_message("test-2", raw_domain_message("test-2", "test-event-2"))
            .await
            .expect("retain reset evidence");
        assert!(control_budget.retained_bytes() > 0);
        hub.retain_dispatch_correlations();
        assert_eq!(control_budget.retained_bytes(), 0);
        assert_eq!(dispatch_budget.retained_bytes(), dispatch_only);

        register_mode(&hub, "drop-1", OperationMode::Dispatch);
        hub.observe_message("drop-1", raw_domain_message("drop-1", "drop-event-1"))
            .await
            .expect("retain drop evidence");
        assert!(dispatch_budget.retained_bytes() > dispatch_only);
        drop(hub);
        assert_eq!(control_budget.retained_bytes(), 0);
        assert_eq!(dispatch_budget.retained_bytes(), 0);
    }

    #[tokio::test]
    async fn feed_retention_does_not_evict_raw_observations() {
        let hub = test_hub(1, 2, MAXIMUM_TOTAL_RAW_EVIDENCE_BYTES);
        register(&hub, "correlation-1");
        let observer = hub.observer(OperationMode::Test, Arc::new(crate::RedactTracePayloads));
        for index in 0..3 {
            observer
                .observe_domain_event(
                    "correlation-1",
                    DomainEventObservation::new(format!("event-{index}"), "bicycle-rented", 1)
                        .with_payload(json!({ "index": index })),
                )
                .await
                .expect("observe retained raw event");
        }

        assert_eq!(
            hub.observed_message_series("correlation-1")
                .await
                .expect("raw message series")
                .messages()
                .len(),
            3
        );
        assert!(matches!(
            hub.subscribe("correlation-1", 0).await,
            Err(CorrelationError::ExpiredCursor { oldest: 3 })
        ));
    }

    #[tokio::test]
    async fn active_subscription_closes_instead_of_skipping_evicted_events() {
        let hub = test_hub(1, 2, MAXIMUM_TOTAL_RAW_EVIDENCE_BYTES);
        hub.register_command(
            "correlation-1",
            OperationMode::Test,
            "operation-1".to_owned(),
            "test-command".to_owned(),
            1,
            "test-context/test-aggregate".to_owned(),
            "aggregate-1".to_owned(),
        )
        .expect("register correlation");
        let mut subscription = hub
            .subscribe("correlation-1", 0)
            .await
            .expect("correlation subscription");
        for operation_id in ["operation-1", "operation-2"] {
            hub.command_result(
                "correlation-1",
                operation_id.to_owned(),
                CorrelationCommandOutcome::Accepted,
                None,
            )
            .await
            .expect("record command result");
        }

        assert_eq!(subscription.next().await, None);
        assert!(matches!(
            hub.subscribe("correlation-1", 0).await,
            Err(CorrelationError::ExpiredCursor { oldest: 2 })
        ));
    }
}
