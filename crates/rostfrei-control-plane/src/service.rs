use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rostfrei_core::{AggregateId, ContentFingerprint, EventHistory, OperationId};
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    operation::{subscribe, NewOperation, OperationRecord},
    runtime::{
        stream_id, CommandKey, ErasedCommandSimulator, RuntimeBindings, RuntimeDecision,
        RuntimeSimulationError,
    },
    CommandWireCodec, OperationEventKind, OperationResult, OperationSnapshot,
    OperationSubscription, PredictedDomainEvent, RuntimeRegistrationError, SubscriptionError,
};

pub const MAX_COMMAND_PAYLOAD_LEN: usize = 1024 * 1024;
const DEFAULT_MAXIMUM_OPERATIONS: usize = 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationRequest {
    pub schema_version: u32,
    pub payload: Value,
}

pub trait TracePayloadPolicy: Send + Sync {
    fn domain_event(&self, event: PredictedDomainEvent) -> PredictedDomainEvent;

    fn rejection(&self, rejection: Value) -> Value;

    fn failure_message(&self, message: String) -> String;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RedactTracePayloads;

impl TracePayloadPolicy for RedactTracePayloads {
    fn domain_event(&self, mut event: PredictedDomainEvent) -> PredictedDomainEvent {
        event.payload = None;
        event.payload_base64 = None;
        event
    }

    fn rejection(&self, _rejection: Value) -> Value {
        serde_json::json!({ "redacted": true })
    }

    fn failure_message(&self, _message: String) -> String {
        "simulation failure details are redacted".to_owned()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExposeTracePayloadsForLocalDevelopment;

impl TracePayloadPolicy for ExposeTracePayloadsForLocalDevelopment {
    fn domain_event(&self, event: PredictedDomainEvent) -> PredictedDomainEvent {
        event
    }

    fn rejection(&self, rejection: Value) -> Value {
        rejection
    }

    fn failure_message(&self, message: String) -> String {
        message
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum SubmissionError {
    #[error(
        "unknown command `{command}` version {schema_version} for aggregate `{aggregate_type}`"
    )]
    UnknownCommand {
        aggregate_type: String,
        command: String,
        schema_version: u32,
    },
    #[error("invalid aggregate identity: {0}")]
    InvalidAggregateId(String),
    #[error("invalid operation identity: {0}")]
    InvalidOperationId(String),
    #[error("command payload exceeds its {maximum}-byte limit")]
    PayloadTooLarge { maximum: usize },
    #[error("operation identity was reused for a different request")]
    IdentityConflict,
    #[error("operation capacity is exhausted")]
    CapacityExhausted,
    #[error("operation was not found")]
    NotFound,
    #[error(transparent)]
    InvalidCursor(#[from] SubscriptionError),
}

pub struct ControlPlaneBuilder {
    history: Arc<dyn EventHistory>,
    bindings: RuntimeBindings,
    maximum_operations: usize,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
}

impl ControlPlaneBuilder {
    pub fn new(history: Arc<dyn EventHistory>, registry: DomainRegistry) -> Self {
        Self {
            history,
            bindings: RuntimeBindings::new(registry),
            maximum_operations: DEFAULT_MAXIMUM_OPERATIONS,
            trace_payload_policy: Arc::new(RedactTracePayloads),
        }
    }

    #[must_use]
    pub fn with_maximum_operations(mut self, maximum_operations: usize) -> Self {
        self.maximum_operations = maximum_operations;
        self
    }

    #[must_use]
    pub fn with_trace_payload_policy(
        mut self,
        trace_payload_policy: Arc<dyn TracePayloadPolicy>,
    ) -> Self {
        self.trace_payload_policy = trace_payload_policy;
        self
    }

    pub fn register<Command, Wire>(
        &mut self,
        wire_codec: Wire,
    ) -> Result<&mut Self, RuntimeRegistrationError>
    where
        Command: CommandDefinition,
        <Command::Aggregate as rostfrei_core::Aggregate>::State: Send,
        <Command::Aggregate as rostfrei_core::Aggregate>::Event: rostfrei_core::Event + Send,
        Wire: CommandWireCodec<Command> + 'static,
    {
        self.bindings.register::<Command, Wire>(wire_codec)?;
        Ok(self)
    }

    pub fn register_with_codec<Command, Codec, Wire>(
        &mut self,
        event_codec: Codec,
        wire_codec: Wire,
    ) -> Result<&mut Self, RuntimeRegistrationError>
    where
        Command: CommandDefinition,
        <Command::Aggregate as rostfrei_core::Aggregate>::State: Send,
        <Command::Aggregate as rostfrei_core::Aggregate>::Event: Send,
        Codec: rostfrei_core::EventCodec<Command::Aggregate> + Clone + Send + Sync + 'static,
        Wire: CommandWireCodec<Command> + 'static,
    {
        self.bindings
            .register_with_codec::<Command, Codec, Wire>(event_codec, wire_codec)?;
        Ok(self)
    }

    pub fn build(self) -> Result<ControlPlane, RuntimeRegistrationError> {
        self.bindings.validate()?;
        Ok(ControlPlane {
            inner: Arc::new(ControlPlaneInner {
                history: self.history,
                simulators: self.bindings.simulators,
                operations: Mutex::new(OperationTable::default()),
                maximum_operations: self.maximum_operations,
                generated_ids: AtomicU64::new(0),
                trace_payload_policy: self.trace_payload_policy,
            }),
        })
    }
}

struct ControlPlaneInner {
    history: Arc<dyn EventHistory>,
    simulators: HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
    operations: Mutex<OperationTable>,
    maximum_operations: usize,
    generated_ids: AtomicU64,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
}

#[derive(Default)]
struct OperationTable {
    records: HashMap<String, Arc<OperationRecord>>,
    insertion_order: VecDeque<String>,
}

impl OperationTable {
    fn evict_terminal(&mut self) -> bool {
        for _ in 0..self.insertion_order.len() {
            let operation_id = self
                .insertion_order
                .pop_front()
                .expect("the bounded scan starts with a non-empty queue");
            if self
                .records
                .get(&operation_id)
                .is_some_and(|record| record.is_terminal())
            {
                self.records.remove(&operation_id);
                return true;
            }
            self.insertion_order.push_back(operation_id);
        }
        false
    }
}

#[derive(Clone)]
pub struct ControlPlane {
    inner: Arc<ControlPlaneInner>,
}

impl ControlPlane {
    pub async fn submit_simulation(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        command: &str,
        request: SimulationRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OperationSnapshot, SubmissionError> {
        let key = CommandKey::new(command, request.schema_version);
        let simulator = self
            .inner
            .simulators
            .get(&key)
            .filter(|simulator| simulator.descriptor().aggregate_type == aggregate_type)
            .cloned()
            .ok_or_else(|| SubmissionError::UnknownCommand {
                aggregate_type: aggregate_type.to_owned(),
                command: command.to_owned(),
                schema_version: request.schema_version,
            })?;
        let aggregate_id = AggregateId::new(aggregate_id)
            .map_err(|error| SubmissionError::InvalidAggregateId(error.to_string()))?;
        let operation_id = match idempotency_key {
            Some(value) => validate_http_operation_id(value)?,
            None => self.generated_operation_id()?,
        };
        let request_bytes = serde_json::to_vec(&request.payload)
            .expect("serde_json::Value always serializes successfully");
        if request_bytes.len() > MAX_COMMAND_PAYLOAD_LEN {
            return Err(SubmissionError::PayloadTooLarge {
                maximum: MAX_COMMAND_PAYLOAD_LEN,
            });
        }
        let fingerprint = request_fingerprint(
            aggregate_type,
            aggregate_id.as_str(),
            command,
            request.schema_version,
            &request_bytes,
        );
        let operation_key = operation_id.as_str().to_owned();
        let record = OperationRecord::new(NewOperation {
            operation_id: operation_key.clone(),
            fingerprint: fingerprint.to_hex(),
            command,
            schema_version: request.schema_version,
            aggregate_type,
            aggregate_id: aggregate_id.as_str(),
        });

        {
            let mut operations = self.inner.operations.lock().await;
            if let Some(existing) = operations.records.get(&operation_key) {
                if existing.fingerprint().await != fingerprint.to_hex() {
                    return Err(SubmissionError::IdentityConflict);
                }
                return Ok(existing.snapshot().await);
            }
            if operations.records.len() >= self.inner.maximum_operations
                && !operations.evict_terminal()
            {
                return Err(SubmissionError::CapacityExhausted);
            }
            operations.insertion_order.push_back(operation_key.clone());
            operations
                .records
                .insert(operation_key, Arc::clone(&record));
        }

        let queued = record.snapshot().await;
        let control_plane = self.clone();
        let panic_record = Arc::clone(&record);
        let execution = tokio::spawn(async move {
            control_plane
                .run_simulation(
                    record,
                    simulator,
                    aggregate_id,
                    fingerprint,
                    request.payload,
                )
                .await;
        });
        tokio::spawn(async move {
            if execution.await.is_err() {
                panic_record
                    .fail(
                        "simulation-panicked",
                        "the command simulation task panicked".to_owned(),
                    )
                    .await;
            }
        });
        Ok(queued)
    }

    pub async fn operation(
        &self,
        operation_id: &str,
    ) -> Result<OperationSnapshot, SubmissionError> {
        let record = self.record(operation_id).await?;
        Ok(record.snapshot().await)
    }

    pub async fn subscribe(
        &self,
        operation_id: &str,
        after: u64,
    ) -> Result<OperationSubscription, SubmissionError> {
        let record = self.record(operation_id).await?;
        Ok(subscribe(&record, after).await?)
    }

    async fn record(&self, operation_id: &str) -> Result<Arc<OperationRecord>, SubmissionError> {
        OperationId::new(operation_id)
            .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))?;
        self.inner
            .operations
            .lock()
            .await
            .records
            .get(operation_id)
            .cloned()
            .ok_or(SubmissionError::NotFound)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_simulation(
        &self,
        record: Arc<OperationRecord>,
        simulator: Arc<dyn ErasedCommandSimulator>,
        aggregate_id: AggregateId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) {
        record.start().await;
        let stream = match stream_id(simulator.descriptor(), aggregate_id) {
            Ok(stream) => stream,
            Err(error) => {
                record
                    .fail(
                        "invalid-runtime",
                        self.inner
                            .trace_payload_policy
                            .failure_message(error.to_string()),
                    )
                    .await;
                return;
            }
        };
        match simulator
            .simulate(
                Arc::clone(&self.inner.history),
                stream,
                simulation_execution_id(fingerprint),
                fingerprint,
                payload,
            )
            .await
        {
            Ok(RuntimeDecision::Accepted {
                base_stream_version,
                events,
            }) => {
                let events = events
                    .into_iter()
                    .map(|event| self.inner.trace_payload_policy.domain_event(event))
                    .collect();
                complete_accepted(&record, base_stream_version, events).await;
            }
            Ok(RuntimeDecision::Rejected {
                base_stream_version,
                rejection,
            }) => {
                complete_rejected(
                    &record,
                    base_stream_version,
                    self.inner.trace_payload_policy.rejection(rejection),
                )
                .await;
            }
            Err(error) => {
                let (code, message) = runtime_failure(error);
                record
                    .fail(
                        code,
                        self.inner.trace_payload_policy.failure_message(message),
                    )
                    .await;
            }
        }
    }

    fn generated_operation_id(&self) -> Result<OperationId, SubmissionError> {
        let sequence = self.inner.generated_ids.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        OperationId::new(format!("simulation-{nanos:x}-{sequence:x}"))
            .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))
    }
}

fn validate_http_operation_id(value: &str) -> Result<OperationId, SubmissionError> {
    let operation_id = OperationId::new(value)
        .map_err(|error| SubmissionError::InvalidOperationId(error.to_string()))?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(SubmissionError::InvalidOperationId(
            "idempotency key must use only ASCII letters, digits, '-', '_', '.', or ':'".to_owned(),
        ));
    }
    Ok(operation_id)
}

fn simulation_execution_id(fingerprint: ContentFingerprint) -> OperationId {
    OperationId::new(format!("simulation:{}", fingerprint.to_hex()))
        .expect("a fingerprint-based simulation operation ID is valid")
}

async fn complete_accepted(
    record: &OperationRecord,
    base_stream_version: u64,
    events: Vec<PredictedDomainEvent>,
) {
    let mut trace = vec![
        OperationEventKind::HistoryReplayed {
            base_stream_version,
        },
        OperationEventKind::CommandAccepted,
    ];
    trace.extend(
        events
            .iter()
            .cloned()
            .map(|event| OperationEventKind::PredictedDomainEvent { event }),
    );
    record
        .complete(
            OperationResult::Accepted {
                base_stream_version,
                predicted_events: events,
                appended: false,
                published: false,
            },
            trace,
        )
        .await;
}

async fn complete_rejected(record: &OperationRecord, base_stream_version: u64, rejection: Value) {
    record
        .complete(
            OperationResult::Rejected {
                base_stream_version,
                rejection: rejection.clone(),
                appended: false,
                published: false,
            },
            vec![
                OperationEventKind::HistoryReplayed {
                    base_stream_version,
                },
                OperationEventKind::CommandRejected { rejection },
            ],
        )
        .await;
}

fn runtime_failure(error: RuntimeSimulationError) -> (&'static str, String) {
    match error {
        RuntimeSimulationError::InvalidPayload(error) => {
            ("invalid-command-payload", error.to_string())
        }
        RuntimeSimulationError::Simulation(error) => ("simulation-failed", error),
        RuntimeSimulationError::RejectionEncoding(error) => {
            ("rejection-encoding-failed", error.to_string())
        }
        RuntimeSimulationError::StreamVersionOverflow => {
            ("stream-version-overflow", error.to_string())
        }
    }
}

fn request_fingerprint(
    aggregate_type: &str,
    aggregate_id: &str,
    command: &str,
    schema_version: u32,
    payload: &[u8],
) -> ContentFingerprint {
    let mut framed = Vec::new();
    for value in [
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
        command.as_bytes(),
    ] {
        framed.extend_from_slice(&value.len().to_be_bytes());
        framed.extend_from_slice(value);
    }
    framed.extend_from_slice(&schema_version.to_be_bytes());
    framed.extend_from_slice(&payload.len().to_be_bytes());
    framed.extend_from_slice(payload);
    ContentFingerprint::digest(framed)
}
