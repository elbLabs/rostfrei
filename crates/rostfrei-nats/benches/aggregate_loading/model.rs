use std::{collections::BTreeMap, convert::Infallible};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, CommandDecision, CommandExecution,
    CommandExecutionMetadata, CommandHandler, CommandHandlingResult, ContentFingerprint, Event,
    EventCodecError, EventCodecErrorKind, OperationId, RecordedEvent, StreamId,
};
use rostfrei_messaging_core::BoundedContextName;
use serde::{Deserialize, Serialize};

use super::BenchResult;

pub const CONTEXT: &str = "aggregate-benchmark";
const SKU_COUNT: u64 = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StockReceived {
    pub sku: u64,
    pub quantity: u64,
    pub note: String,
}

impl StockReceived {
    pub fn at(index: u64, note_bytes: u32) -> BenchResult<Self> {
        Ok(Self {
            sku: index.rem_euclid(SKU_COUNT),
            quantity: 1,
            note: "x".repeat(usize::try_from(note_bytes)?),
        })
    }
}

impl Event for StockReceived {
    fn event_type(&self) -> &'static str {
        "stock-received"
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        serde_json::to_vec(self).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
        })
    }

    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        if event.event_type() != "stock-received" {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnknownEventType,
                "unknown benchmark event",
            ));
        }
        if event.schema_version() != 1 {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "unsupported benchmark event version",
            ));
        }
        serde_json::from_slice(event.payload()).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
        })
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Inventory {
    stock: BTreeMap<u64, u64>,
    received: u64,
    note_bytes: u64,
}

pub struct InventoryAggregate;

impl Aggregate for InventoryAggregate {
    type State = Inventory;
    type Event = StockReceived;

    const BOUNDED_CONTEXT: &'static str = CONTEXT;
    const AGGREGATE_TYPE: &'static str = "benchmark-inventory";

    fn initial(_stream_id: &StreamId) -> Inventory {
        Inventory::default()
    }

    fn apply(state: &mut Inventory, event: &StockReceived) {
        let quantity = state.stock.entry(event.sku).or_default();
        *quantity = quantity.saturating_add(event.quantity);
        state.received = state.received.saturating_add(1);
        state.note_bytes = state
            .note_bytes
            .saturating_add(u64::try_from(event.note.len()).unwrap_or(u64::MAX));
    }
}

#[derive(Serialize)]
pub struct ReceiveStock {
    pub inventory_id: String,
    pub event: StockReceived,
}

pub struct ReceiveStockHandler;

#[async_trait]
impl CommandHandler<ReceiveStock> for ReceiveStockHandler {
    type Rejection = Infallible;

    async fn handle(
        &self,
        command: &ReceiveStock,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Infallible> {
        let mut inventory = execution
            .load::<InventoryAggregate>(&command.inventory_id)
            .await?;
        inventory.aggregate_mut().raise(command.event.clone());
        Ok(CommandDecision::Accepted)
    }
}

pub fn stream(id: &str) -> BenchResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new(InventoryAggregate::AGGREGATE_TYPE)?,
        AggregateId::new(id)?,
    ))
}

pub fn metadata(operation: &str, command: &ReceiveStock) -> BenchResult<CommandExecutionMetadata> {
    Ok(CommandExecutionMetadata::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(serde_json::to_vec(command)?),
    )
    .with_bounded_context(BoundedContextName::new(CONTEXT)?))
}

pub fn verify(state: &Inventory, expected_events: u64, note_bytes: u32) -> BenchResult {
    let expected_bytes = expected_events
        .checked_mul(u64::from(note_bytes))
        .ok_or("expected byte count overflow")?;
    if state.received != expected_events
        || state.note_bytes != expected_bytes
        || state.stock.len() != usize::try_from(expected_events.min(SKU_COUNT))?
    {
        return Err("aggregate state does not match the benchmark history".into());
    }
    for (&sku, &actual) in &state.stock {
        let base = expected_events
            .checked_div(SKU_COUNT)
            .ok_or("invalid SKU count")?;
        let extra = u64::from(sku < expected_events.rem_euclid(SKU_COUNT));
        if sku >= SKU_COUNT || Some(actual) != base.checked_add(extra) {
            return Err("aggregate stock projection is incorrect".into());
        }
    }
    Ok(())
}
