//! Domain fixture shared by the independent modular-runtime acceptance targets.
#![allow(dead_code, unused_imports)]
use async_trait::async_trait;
pub use rostfrei_core::*;
pub use rostfrei_runtime::{
    ApplicationErrorCode, ApplicationName, BoundedContext, CommandRejection,
    CommandRejectionClassification, CommandResponseOutcome,
};
pub use rostfrei_runtime::{
    CommandBus, CommandProcessor, CommandRequest, EncodedCommand, InMemoryMessagingAdapter,
};
pub use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(rostfrei_runtime::BoundedContext)]
#[domain(id = "modular", label = "Modular contract")]
pub struct Modular;

#[derive(Clone, Debug, rostfrei_runtime::Command)]
#[domain(context = Modular, id = "change", label = "Change")]
pub struct Change {
    pub amount: i64,
    pub reject: bool,
}

pub struct Counter;
impl Aggregate for Counter {
    type State = i64;
    type Event = Added;
    const BOUNDED_CONTEXT: &'static str = "modular";
    const AGGREGATE_TYPE: &'static str = "counter";
    fn initial(_: &StreamId) -> i64 {
        0
    }
    fn apply(state: &mut i64, event: &Added) {
        *state = state.saturating_add(event.0);
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Added(pub i64);
impl Event for Added {
    fn event_type(&self) -> &'static str {
        "added"
    }
    fn schema_version(&self) -> u32 {
        2
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        serde_json::to_vec(self)
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::EncodingFailed, e.to_string()))
    }
    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        serde_json::from_slice(event.payload())
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::MalformedPayload, e.to_string()))
    }
}

/// Genuine historical semantics: v1 stored whole units, v2 stores signed little-endian cents.
/// Neither wire representation is the generated JSON representation of Added.
#[derive(Clone)]
pub struct CentsCodec {
    pub scale: i64,
    pub decodes: Arc<AtomicUsize>,
}
impl EventCodec<Counter> for CentsCodec {
    fn encode(&self, event: &Added, id: EventId) -> Result<NewEvent, EventCodecError> {
        NewEvent::new(id, "added", 2, event.0.to_le_bytes().to_vec())
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, e.to_string()))
    }
    fn decode(&self, event: &RecordedEvent) -> Result<Added, EventCodecError> {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        if event.event_type() != "added" {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnknownEventType,
                "expected added",
            ));
        }
        let value = match event.schema_version() {
            1 => std::str::from_utf8(event.payload())
                .ok()
                .and_then(|s| s.strip_prefix("units:"))
                .and_then(|s| s.parse::<i64>().ok())
                .map(|n| n.saturating_mul(self.scale)),
            2 => <[u8; 8]>::try_from(event.payload())
                .ok()
                .map(i64::from_le_bytes),
            _ => {
                return Err(EventCodecError::new(
                    EventCodecErrorKind::UnsupportedSchemaVersion,
                    "unsupported added schema",
                ));
            }
        };
        value.map(Added).ok_or_else(|| {
            EventCodecError::new(
                EventCodecErrorKind::MalformedPayload,
                "malformed cents history",
            )
        })
    }
}
pub struct ChangeHandler(pub Arc<AtomicUsize>);
#[async_trait]
impl CommandHandler<Change> for ChangeHandler {
    type Rejection = String;
    async fn handle(
        &self,
        command: &Change,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let mut counter = execution.load::<Counter>("one").await?;
        if command.reject {
            return Ok(CommandDecision::Rejected(format!(
                "balance={}",
                counter.state()
            )));
        }
        counter.raise(Added(command.amount));
        Ok(CommandDecision::Accepted)
    }
}
pub fn map_rejection(value: &String) -> Result<CommandRejection, String> {
    CommandRejection::new(
        CommandRejectionClassification::Conflict,
        ApplicationErrorCode::new("modular.refused").map_err(|e| e.to_string())?,
        value,
        None,
    )
    .map_err(|e| e.to_string())
}
pub fn context() -> TestResult<BoundedContext> {
    Ok(ApplicationName::new("modular-tests")?.bounded_context("modular")?)
}
pub fn stream() -> TestResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new("counter")?,
        AggregateId::new("one")?,
    ))
}
pub fn request(id: &str, reject: bool) -> TestResult<CommandRequest<Change>> {
    Ok(
        CommandRequest::new(OperationId::new(id)?, Change { amount: 7, reject }).with_created_at(
            rostfrei_runtime::MessageTimestamp::from_unix_milliseconds(1_700_000_000_000)?,
        ),
    )
}
pub fn noncanonical(command: &EncodedCommand) -> TestResult<EncodedCommand> {
    let value: serde_json::Value = serde_json::from_slice(command.payload())?;
    let mut original = serde_json::to_vec_pretty(&value)?;
    original.extend_from_slice(b"\n \t");
    assert_ne!(
        original,
        command.payload(),
        "fixture must detect reserialization"
    );
    let original = EncodedCommand::from_delivery(
        command.address().clone(),
        command.message_id().clone(),
        original,
    )?;
    original.validate()?;
    Ok(original)
}
pub fn baseline_processor(
    store: Arc<dyn ExecutionStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<Arc<CommandProcessor>> {
    let mut processor = CommandProcessor::new(context()?.name().clone(), store);
    processor.register::<Change, _>(ChangeHandler(calls), map_rejection)?;
    Ok(Arc::new(processor))
}
pub fn bus(processor: Arc<CommandProcessor>) -> TestResult<CommandBus> {
    Ok(CommandBus::new(
        context()?,
        Arc::new(InMemoryMessagingAdapter::new(processor)),
    ))
}
pub async fn seed_v1(store: &impl EventStore) -> TestResult<Vec<RecordedEvent>> {
    let operation = OperationId::new("historical-operation")?;
    let commit = derive_commit_id(&stream()?, &operation);
    let event = derive_event_id(&commit, 0);
    Ok(store
        .append(
            &stream()?,
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit,
                operation,
                ContentFingerprint::digest("historical"),
                vec![NewEvent::new(event, "added", 1, b"units:2".to_vec())?],
            )?,
        )
        .await?
        .into_events())
}
