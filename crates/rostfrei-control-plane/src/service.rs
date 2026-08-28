use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rostfrei_core::{
    AggregateId, ContentFingerprint, EventHistory, EventStoreErrorKind, OperationId,
    SimulationError,
};
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};

use crate::{
    CommandWireCodec, DomainJsonWireCodec, OperationEventKind, OperationResult, OperationSnapshot,
    OperationSubscription, PredictedDomainEvent, RuntimeRegistrationError, SubscriptionError,
    operation::{NewOperation, OperationRecord, subscribe},
    runtime::{
        CommandKey, ErasedCommandSimulator, RuntimeBindings, RuntimeDecision,
        RuntimeSimulationError, stream_id,
    },
};

pub const MAX_COMMAND_PAYLOAD_LEN: usize = 1024 * 1024;
const DEFAULT_MAXIMUM_OPERATIONS: usize = 1024;
const DEFAULT_MAXIMUM_CONCURRENT_SIMULATIONS: usize = 32;

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
    #[error("simulation concurrency is exhausted")]
    ConcurrencyExhausted,
    #[error("operation was not found")]
    NotFound,
    #[error(transparent)]
    InvalidCursor(#[from] SubscriptionError),
}

pub struct ControlPlaneBuilder {
    history: Arc<dyn EventHistory>,
    bindings: RuntimeBindings,
    maximum_operations: usize,
    maximum_concurrent_simulations: usize,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
}

impl ControlPlaneBuilder {
    pub fn new(history: Arc<dyn EventHistory>) -> Self {
        Self::with_registry(history, DomainRegistry::new())
    }

    pub fn with_registry(history: Arc<dyn EventHistory>, registry: DomainRegistry) -> Self {
        Self {
            history,
            bindings: RuntimeBindings::new(registry),
            maximum_operations: DEFAULT_MAXIMUM_OPERATIONS,
            maximum_concurrent_simulations: DEFAULT_MAXIMUM_CONCURRENT_SIMULATIONS,
            trace_payload_policy: Arc::new(RedactTracePayloads),
        }
    }

    #[must_use]
    pub const fn with_maximum_operations(mut self, maximum_operations: usize) -> Self {
        self.maximum_operations = maximum_operations;
        self
    }

    #[must_use]
    pub const fn with_maximum_concurrent_simulations(
        mut self,
        maximum_concurrent_simulations: usize,
    ) -> Self {
        self.maximum_concurrent_simulations = maximum_concurrent_simulations;
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
        Command::Aggregate: rostfrei_core::CommandHandler<Command>,
        <Command::Aggregate as rostfrei_core::Aggregate>::State: Send,
        <Command::Aggregate as rostfrei_core::Aggregate>::Event: rostfrei_core::Event + Send,
        Wire: CommandWireCodec<Command> + 'static,
    {
        self.bindings.register::<Command, Wire>(wire_codec)?;
        Ok(self)
    }

    pub fn register_json<Command>(&mut self) -> Result<&mut Self, RuntimeRegistrationError>
    where
        Command: CommandDefinition + domain::JsonCommandPayload,
        Command::Aggregate: rostfrei_core::CommandHandler<Command>,
        <Command::Aggregate as rostfrei_core::Aggregate>::State: Send,
        <Command::Aggregate as rostfrei_core::Aggregate>::Event: rostfrei_core::Event + Send,
        <Command::Aggregate as rostfrei_core::CommandHandler<Command>>::Rejection:
            domain::JsonErrorPayload,
    {
        self.register::<Command, _>(DomainJsonWireCodec)
    }

    pub fn register_with_codec<Command, Codec, Wire>(
        &mut self,
        event_codec: Codec,
        wire_codec: Wire,
    ) -> Result<&mut Self, RuntimeRegistrationError>
    where
        Command: CommandDefinition,
        Command::Aggregate: rostfrei_core::CommandHandler<Command>,
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
        let maximum_concurrent_simulations = self
            .maximum_concurrent_simulations
            .min(self.maximum_operations)
            .min(Semaphore::MAX_PERMITS);
        Ok(ControlPlane {
            inner: Arc::new(ControlPlaneInner {
                history: self.history,
                simulators: self.bindings.simulators,
                operations: Mutex::new(OperationTable::default()),
                maximum_operations: self.maximum_operations,
                simulation_permits: Arc::new(Semaphore::new(maximum_concurrent_simulations)),
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
    simulation_permits: Arc<Semaphore>,
    generated_ids: AtomicU64,
    trace_payload_policy: Arc<dyn TracePayloadPolicy>,
}

#[derive(Default)]
struct OperationTable {
    records: HashMap<String, Arc<OperationRecord>>,
    insertion_order: VecDeque<String>,
}

impl OperationTable {
    fn has_terminal(&self) -> bool {
        self.records.values().any(|record| record.is_terminal())
    }

    fn evict_terminal(&mut self) {
        self.rebuild_insertion_order();
        let terminal = self.insertion_order.iter().find_map(|operation_id| {
            self.records
                .get(operation_id)
                .is_some_and(|record| record.is_terminal())
                .then(|| operation_id.clone())
        });
        if let Some(operation_id) = terminal {
            self.records.remove(&operation_id);
            self.insertion_order
                .retain(|queued_id| queued_id != &operation_id);
        }
    }

    fn rebuild_insertion_order(&mut self) {
        let mut queued = HashSet::with_capacity(self.records.len());
        let records = &self.records;
        self.insertion_order.retain(|operation_id| {
            records.contains_key(operation_id) && queued.insert(operation_id.clone())
        });
        for operation_id in self.records.keys() {
            if queued.insert(operation_id.clone()) {
                self.insertion_order.push_back(operation_id.clone());
            }
        }
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
        let key = CommandKey::new(aggregate_type, command, request.schema_version);
        let simulator = self.inner.simulators.get(&key).cloned().ok_or_else(|| {
            SubmissionError::UnknownCommand {
                aggregate_type: aggregate_type.to_owned(),
                command: command.to_owned(),
                schema_version: request.schema_version,
            }
        })?;
        let aggregate_id = AggregateId::new(aggregate_id)
            .map_err(|error| SubmissionError::InvalidAggregateId(error.to_string()))?;
        let operation_id = match idempotency_key {
            Some(value) => validate_http_operation_id(value)?,
            None => self.generated_operation_id()?,
        };
        let request_bytes = compact_json_bytes(&request.payload);
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

        let permit = {
            let mut operations = self.inner.operations.lock().await;
            if let Some(existing) = operations.records.get(&operation_key) {
                if existing.fingerprint().await != fingerprint.to_hex() {
                    return Err(SubmissionError::IdentityConflict);
                }
                return Ok(existing.snapshot().await);
            }
            if operations.records.len() >= self.inner.maximum_operations
                && !operations.has_terminal()
            {
                return Err(SubmissionError::CapacityExhausted);
            }
            let permit = Arc::clone(&self.inner.simulation_permits)
                .try_acquire_owned()
                .map_err(|_| SubmissionError::ConcurrencyExhausted)?;
            if operations.records.len() >= self.inner.maximum_operations {
                operations.evict_terminal();
            }
            operations.insertion_order.push_back(operation_key.clone());
            operations
                .records
                .insert(operation_key, Arc::clone(&record));
            permit
        };

        let queued = record.snapshot().await;
        let control_plane = self.clone();
        let panic_record = Arc::clone(&record);
        let execution = tokio::spawn(async move {
            let _permit = permit;
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
        let execution_id = match simulation_execution_id(fingerprint) {
            Ok(execution_id) => execution_id,
            Err(message) => {
                record
                    .fail(
                        "invalid-runtime",
                        self.inner.trace_payload_policy.failure_message(message),
                    )
                    .await;
                return;
            }
        };
        match simulator
            .simulate(
                Arc::clone(&self.inner.history),
                stream,
                execution_id,
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

fn simulation_execution_id(fingerprint: ContentFingerprint) -> Result<OperationId, String> {
    OperationId::new(format!("simulation:{}", fingerprint.to_hex()))
        .map_err(|error| error.to_string())
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
        RuntimeSimulationError::Simulation(SimulationError::Codec(error)) => {
            ("event-codec-failed", error.to_string())
        }
        RuntimeSimulationError::Simulation(SimulationError::Store(error)) => {
            let code = match error.kind() {
                EventStoreErrorKind::CorruptHistory => "corrupt-history",
                EventStoreErrorKind::Unavailable
                | EventStoreErrorKind::CapacityExhausted
                | EventStoreErrorKind::ConfigurationMismatch => "history-unavailable",
                EventStoreErrorKind::Conflict | EventStoreErrorKind::IdentityConflict => {
                    "history-conflict"
                }
                EventStoreErrorKind::InvalidRequest => "invalid-runtime",
            };
            (code, error.to_string())
        }
        RuntimeSimulationError::RejectionEncoding(error) => {
            ("rejection-encoding-failed", error.to_string())
        }
        RuntimeSimulationError::StreamVersionOverflow => {
            ("stream-version-overflow", error.to_string())
        }
    }
}

fn compact_json_bytes(payload: &Value) -> Vec<u8> {
    payload.to_string().into_bytes()
}

fn request_fingerprint(
    aggregate_type: &str,
    aggregate_id: &str,
    command: &str,
    schema_version: u32,
    payload: &[u8],
) -> ContentFingerprint {
    let mut framed = Vec::new();
    let schema_version = schema_version.to_be_bytes();
    for value in [
        b"rostfrei:simulation-request:v1".as_slice(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
        command.as_bytes(),
        schema_version.as_slice(),
        payload,
    ] {
        framed.extend_from_slice(&bounded_length_bytes(value.len()));
        framed.extend_from_slice(value);
    }
    ContentFingerprint::digest(framed)
}

fn bounded_length_bytes(length: usize) -> [u8; 8] {
    // Fingerprint parts are bounded by registry identities or MAX_COMMAND_PAYLOAD_LEN.
    let mut encoded = [0_u8; 8];
    for (target, source) in encoded
        .iter_mut()
        .rev()
        .zip(length.to_be_bytes().iter().rev())
    {
        *target = *source;
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_fingerprints_use_deterministic_fixed_width_framing() {
        let request_bytes = compact_json_bytes(&serde_json::json!({
            "bicycle_id": "bike-42",
        }));
        assert_eq!(request_bytes, br#"{"bicycle_id":"bike-42"}"#);
        let fingerprint = request_fingerprint(
            "bike-rental/rental-fleet",
            "city-fleet",
            "rent-bicycle",
            1,
            &request_bytes,
        );

        assert_eq!(
            fingerprint.to_hex(),
            "6e05cbaf829bc0bfa276ca081d504f8c1c234577c46d1b3a49ab8d8a38b2d4c9"
        );
    }

    #[tokio::test]
    async fn terminal_eviction_repairs_a_record_missing_from_the_queue() {
        let record = OperationRecord::new(NewOperation {
            operation_id: "terminal-operation".to_owned(),
            fingerprint: "fingerprint".to_owned(),
            command: "test-command",
            schema_version: 1,
            aggregate_type: "test-context/test-aggregate",
            aggregate_id: "aggregate-1",
        });
        record.fail("test-failure", "test failure".to_owned()).await;
        let mut table = OperationTable::default();
        table
            .records
            .insert("terminal-operation".to_owned(), record);

        assert!(table.has_terminal());
        table.evict_terminal();
        assert!(!table.records.contains_key("terminal-operation"));
        assert!(table.insertion_order.is_empty());
    }
}
