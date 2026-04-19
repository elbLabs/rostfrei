use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, watch};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

impl OperationStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompletedDecision {
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictedDomainEvent {
    pub ordinal: u32,
    pub predicted_stream_version: u64,
    pub event_type: String,
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_base64: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "decision",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OperationResult {
    Accepted {
        #[serde(skip_serializing_if = "Option::is_none")]
        base_stream_version: Option<u64>,
        predicted_events: Vec<PredictedDomainEvent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        appended: Option<bool>,
        published: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        command_message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duplicate: Option<bool>,
    },
    Rejected {
        #[serde(skip_serializing_if = "Option::is_none")]
        base_stream_version: Option<u64>,
        rejection: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        appended: Option<bool>,
        published: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        command_message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duplicate: Option<bool>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationSnapshot {
    pub operation_id: String,
    pub mode: &'static str,
    pub status: OperationStatus,
    pub command: String,
    pub schema_version: u32,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub latest_event_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<OperationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<OperationFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationFailure {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationEvent {
    pub id: u64,
    pub operation_id: String,
    #[serde(flatten)]
    pub kind: OperationEventKind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum OperationEventKind {
    Queued,
    Started,
    HistoryReplayed {
        base_stream_version: u64,
    },
    CommandAccepted,
    PredictedDomainEvent {
        event: PredictedDomainEvent,
    },
    CommandRejected {
        rejection: Value,
    },
    CommandPublished {
        command_message_id: String,
        duplicate: bool,
    },
    CommandResponded {
        response_message_id: String,
    },
    Completed {
        decision: CompletedDecision,
    },
    Failed {
        code: &'static str,
        message: String,
    },
}

impl OperationEventKind {
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::Queued => "operation.queued",
            Self::Started => "operation.started",
            Self::HistoryReplayed { .. } => "history.replayed",
            Self::CommandAccepted => "command.accepted",
            Self::PredictedDomainEvent { .. } => "domain-event.predicted",
            Self::CommandRejected { .. } => "command.rejected",
            Self::CommandPublished { .. } => "command.published",
            Self::CommandResponded { .. } => "command.responded",
            Self::Completed { .. } => "operation.completed",
            Self::Failed { .. } => "operation.failed",
        }
    }
}

pub(crate) struct NewOperation<'a> {
    pub operation_id: String,
    pub fingerprint: String,
    pub command: &'a str,
    pub schema_version: u32,
    pub aggregate_type: &'a str,
    pub aggregate_id: &'a str,
    pub mode: &'static str,
}

struct OperationState {
    fingerprint: String,
    snapshot: OperationSnapshot,
    events: Vec<OperationEvent>,
}

pub(crate) struct OperationRecord {
    state: Mutex<OperationState>,
    changed: watch::Sender<u64>,
    terminal: AtomicBool,
}

impl OperationRecord {
    pub fn new(operation: NewOperation<'_>) -> Arc<Self> {
        let event = OperationEvent {
            id: 1,
            operation_id: operation.operation_id.clone(),
            kind: OperationEventKind::Queued,
        };
        let (changed, _) = watch::channel(1);
        Arc::new(Self {
            state: Mutex::new(OperationState {
                fingerprint: operation.fingerprint,
                snapshot: OperationSnapshot {
                    operation_id: operation.operation_id,
                    mode: operation.mode,
                    status: OperationStatus::Queued,
                    command: operation.command.to_owned(),
                    schema_version: operation.schema_version,
                    aggregate_type: operation.aggregate_type.to_owned(),
                    aggregate_id: operation.aggregate_id.to_owned(),
                    latest_event_id: 1,
                    result: None,
                    failure: None,
                },
                events: vec![event],
            }),
            changed,
            terminal: AtomicBool::new(false),
        })
    }

    pub async fn fingerprint(&self) -> String {
        self.state.lock().await.fingerprint.clone()
    }

    pub async fn snapshot(&self) -> OperationSnapshot {
        self.state.lock().await.snapshot.clone()
    }

    pub async fn start(&self) {
        self.append(OperationEventKind::Started, |snapshot| {
            snapshot.status = OperationStatus::Running;
        })
        .await;
    }

    pub async fn complete(&self, result: OperationResult, events: Vec<OperationEventKind>) {
        let decision = match result {
            OperationResult::Accepted { .. } => CompletedDecision::Accepted,
            OperationResult::Rejected { .. } => CompletedDecision::Rejected,
        };
        let mut state = self.state.lock().await;
        if state.snapshot.status.is_terminal() {
            return;
        }
        for kind in events {
            push_event(&mut state, kind);
        }
        push_event(&mut state, OperationEventKind::Completed { decision });
        state.snapshot.status = OperationStatus::Completed;
        state.snapshot.result = Some(result);
        self.terminal.store(true, Ordering::Release);
        let latest = state.snapshot.latest_event_id;
        drop(state);
        self.changed.send_replace(latest);
    }

    pub async fn fail(&self, code: &'static str, message: String) {
        let mut state = self.state.lock().await;
        if state.snapshot.status.is_terminal() {
            return;
        }
        push_event(
            &mut state,
            OperationEventKind::Failed {
                code,
                message: message.clone(),
            },
        );
        state.snapshot.status = OperationStatus::Failed;
        state.snapshot.failure = Some(OperationFailure { code, message });
        self.terminal.store(true, Ordering::Release);
        let latest = state.snapshot.latest_event_id;
        drop(state);
        self.changed.send_replace(latest);
    }

    pub async fn command_published(&self, command_message_id: String, duplicate: bool) {
        self.append(
            OperationEventKind::CommandPublished {
                command_message_id,
                duplicate,
            },
            |_| {},
        )
        .await;
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal.load(Ordering::Acquire)
    }

    async fn append(&self, kind: OperationEventKind, update: impl FnOnce(&mut OperationSnapshot)) {
        let mut state = self.state.lock().await;
        if state.snapshot.status.is_terminal() {
            return;
        }
        push_event(&mut state, kind);
        update(&mut state.snapshot);
        let latest = state.snapshot.latest_event_id;
        drop(state);
        self.changed.send_replace(latest);
    }

    async fn subscription(
        self: &Arc<Self>,
        after: u64,
    ) -> Result<OperationSubscription, SubscriptionError> {
        let state = self.state.lock().await;
        if after > state.snapshot.latest_event_id {
            return Err(SubscriptionError::FutureCursor {
                latest: state.snapshot.latest_event_id,
            });
        }
        drop(state);
        Ok(OperationSubscription {
            record: Arc::clone(self),
            receiver: self.changed.subscribe(),
            cursor: after,
        })
    }
}

fn push_event(state: &mut OperationState, kind: OperationEventKind) {
    let id = state
        .snapshot
        .latest_event_id
        .checked_add(1)
        .expect("bounded operation traces cannot exhaust u64 event IDs");
    state.events.push(OperationEvent {
        id,
        operation_id: state.snapshot.operation_id.clone(),
        kind,
    });
    state.snapshot.latest_event_id = id;
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SubscriptionError {
    #[error("operation event cursor is ahead of the latest event {latest}")]
    FutureCursor { latest: u64 },
}

pub struct OperationSubscription {
    record: Arc<OperationRecord>,
    receiver: watch::Receiver<u64>,
    cursor: u64,
}

impl OperationSubscription {
    pub async fn is_complete(&self) -> bool {
        let state = self.record.state.lock().await;
        state.snapshot.status.is_terminal() && self.cursor == state.snapshot.latest_event_id
    }

    pub async fn next(&mut self) -> Option<OperationEvent> {
        loop {
            {
                let state = self.record.state.lock().await;
                if let Some(event) = state.events.iter().find(|event| event.id > self.cursor) {
                    self.cursor = event.id;
                    return Some(event.clone());
                }
                if state.snapshot.status.is_terminal() {
                    return None;
                }
            }
            if self.receiver.changed().await.is_err() {
                return None;
            }
        }
    }
}

pub(crate) async fn subscribe(
    record: &Arc<OperationRecord>,
    after: u64,
) -> Result<OperationSubscription, SubscriptionError> {
    record.subscription(after).await
}
