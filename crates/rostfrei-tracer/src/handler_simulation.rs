//! Handler diagnostics, deliberately independent of operations and observed `MessageSeries`.
use rostfrei_core::{CommandDecision, SimulatedParticipant};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::{PredictedDomainEvent, SubmissionError, TouchedStreamParticipant, TracePayloadPolicy};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HandlerSimulationKind {
    HandlerSimulation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "camelCase")]
pub enum HandlerDecision {
    Accepted,
    Rejected { rejection: Value },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedDomainEvent {
    #[serde(skip)]
    ordinal: u32,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub proposed_stream_version: u64,
    pub event_type: String,
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

/// A handler computation, never a publication, durable receipt or observed trace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandlerSimulationResult {
    pub kind: HandlerSimulationKind,
    pub context: String,
    pub command: String,
    pub schema_version: u32,
    pub committed: bool,
    pub outcome: HandlerDecision,
    pub streams: Vec<TouchedStreamParticipant>,
    pub proposed_events: Vec<ProposedDomainEvent>,
}

#[derive(Debug, Error)]
pub enum HandlerSimulationError {
    #[error(transparent)]
    Submission(#[from] SubmissionError),
    #[error("invalid command payload: {0}")]
    InvalidPayload(String),
    #[error("{message}")]
    Runtime { code: &'static str, message: String },
    #[error("handler simulation result exceeds its response budget")]
    ResultTooLarge,
    #[error("handler simulation exceeded its 30-second deadline; dependencies may have run")]
    Timeout,
    #[error("handler simulation failed; dependencies may have run")]
    Failed,
}

impl HandlerSimulationError {
    pub(crate) fn runtime(code: &'static str, error: &(impl ToString + ?Sized)) -> Self {
        Self::Runtime {
            code,
            message: error.to_string(),
        }
    }
}

pub fn result<Rejection: domain::JsonErrorPayload>(
    context: &str,
    command: &str,
    schema_version: u32,
    decision: CommandDecision<Rejection>,
    participants: Vec<SimulatedParticipant>,
) -> Result<HandlerSimulationResult, HandlerSimulationError> {
    let outcome = match decision {
        CommandDecision::Accepted => HandlerDecision::Accepted,
        CommandDecision::Rejected(rejection) => HandlerDecision::Rejected {
            rejection: rejection.encode_json().map_err(|error| {
                HandlerSimulationError::runtime("rejection-encoding-failed", &error)
            })?,
        },
    };
    let mut streams = Vec::new();
    let mut proposed_events = Vec::new();
    for participant in participants {
        let aggregate_type = participant.stream_id().aggregate_type().as_str();
        let aggregate_id = participant.stream_id().aggregate_id().as_str();
        streams.push(TouchedStreamParticipant {
            aggregate_type: aggregate_type.to_owned(),
            aggregate_id: aggregate_id.to_owned(),
            base_stream_version: participant.base_version().value(),
            read_guard: participant.is_read_guard(),
        });
        for (ordinal, event) in participant.events().iter().enumerate() {
            let version = u64::try_from(ordinal)
                .ok()
                .and_then(|ordinal| {
                    participant
                        .base_version()
                        .value()
                        .checked_add(ordinal.checked_add(1)?)
                })
                .ok_or_else(|| {
                    HandlerSimulationError::runtime(
                        "stream-version-overflow",
                        "proposed stream version overflow",
                    )
                })?;
            proposed_events.push(ProposedDomainEvent {
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| HandlerSimulationError::ResultTooLarge)?,
                aggregate_type: aggregate_type.to_owned(),
                aggregate_id: aggregate_id.to_owned(),
                proposed_stream_version: version,
                event_type: event.event_type().to_owned(),
                schema_version: event.schema_version(),
                payload: Some(serde_json::from_slice(event.payload()).map_err(|error| {
                    HandlerSimulationError::runtime("invalid-event-payload", &error)
                })?),
            });
        }
    }
    Ok(HandlerSimulationResult {
        kind: HandlerSimulationKind::HandlerSimulation,
        context: context.to_owned(),
        command: command.to_owned(),
        schema_version,
        committed: false,
        outcome,
        streams,
        proposed_events,
    })
}

impl HandlerSimulationResult {
    pub(crate) fn redact(&mut self, policy: &dyn TracePayloadPolicy) {
        if let HandlerDecision::Rejected { rejection } = &mut self.outcome {
            *rejection = policy.rejection(std::mem::take(rejection));
        }
        for event in &mut self.proposed_events {
            // Reuse the established domain payload policy without exposing legacy trace types.
            let redacted = policy.domain_event(PredictedDomainEvent {
                aggregate_type: event.aggregate_type.clone(),
                aggregate_id: event.aggregate_id.clone(),
                ordinal: event.ordinal,
                predicted_stream_version: event.proposed_stream_version,
                event_type: event.event_type.clone(),
                schema_version: event.schema_version,
                payload: event.payload.take(),
            });
            event.payload = redacted.payload;
            event.aggregate_type = redacted.aggregate_type;
            event.aggregate_id = redacted.aggregate_id;
            event.event_type = redacted.event_type;
            event.schema_version = redacted.schema_version;
            event.proposed_stream_version = redacted.predicted_stream_version;
        }
    }
}
