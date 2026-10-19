//! Private-event schema evolution: current v2 events, explicit v1 readers.
//!
//! v1 `bicycle-registered` stored `needs_repair: bool`; v2 stores a condition.
//! The conversion preserves meaning. v1 `bicycle-retired` recorded no reason, so
//! it becomes the explicit `RetirementReason::Unrecorded` instead of a guess.

use std::collections::BTreeMap;

use async_trait::async_trait;
use rostfrei::{
    Aggregate, AggregateCodecs, BoundedContextName, CommandDecision, CommandExecution,
    CommandExecutionMetadata, CommandHandler, CommandHandlingResult, ContentFingerprint,
    EventBatch, EventCodec, EventCodecError, EventCodecErrorKind, EventId, ExpectedVersion,
    NewEvent, OperationId, RecordedEvent, StreamId,
};
use rostfrei_core::{
    AggregateId, AggregateType, CodecRegistrationError, EventStore, JsonEventCodec,
    derive_commit_id, derive_event_id,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Condition {
    Serviceable,
    MaintenanceRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetirementReason {
    Damaged,
    Sold,
    /// Retired before v2 recorded a reason.
    Unrecorded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[domain(
    id = "bicycle-registered",
    label = "Bicycle registered",
    schema_version = 2
)]
pub struct BicycleRegistered {
    pub bicycle_id: String,
    pub condition: Condition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[domain(id = "bicycle-retired", label = "Bicycle retired", schema_version = 2)]
pub struct BicycleRetired {
    pub bicycle_id: String,
    pub reason: RetirementReason,
}

#[derive(Clone, Debug, Eq, PartialEq, rostfrei::AggregateEvents)]
pub enum InventoryEvent {
    BicycleRegistered(BicycleRegistered),
    BicycleRetired(BicycleRetired),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InventoryState {
    pub active: BTreeMap<String, Condition>,
    pub retired: BTreeMap<String, RetirementReason>,
}

pub struct Inventory;

impl Aggregate for Inventory {
    type State = InventoryState;
    type Event = InventoryEvent;
    const BOUNDED_CONTEXT: &'static str = "fleet-evolution";
    const AGGREGATE_TYPE: &'static str = "fleet-evolution/inventory";

    fn initial(_: &StreamId) -> InventoryState {
        InventoryState::default()
    }

    fn apply(state: &mut InventoryState, event: &InventoryEvent) {
        match event {
            InventoryEvent::BicycleRegistered(event) => {
                state
                    .active
                    .insert(event.bicycle_id.clone(), event.condition);
            }
            InventoryEvent::BicycleRetired(event) => {
                state.active.remove(&event.bicycle_id);
                state.retired.insert(event.bicycle_id.clone(), event.reason);
            }
        }
    }
}

#[derive(Deserialize)]
struct BicycleRegisteredV1 {
    bicycle_id: String,
    needs_repair: bool,
}

#[derive(Deserialize)]
struct BicycleRetiredV1 {
    bicycle_id: String,
}

/// Writes the generated current format and reads every supported historical one.
pub struct InventoryCodec;

impl EventCodec<Inventory> for InventoryCodec {
    fn encode(&self, event: &InventoryEvent, id: EventId) -> Result<NewEvent, EventCodecError> {
        <JsonEventCodec as EventCodec<Inventory>>::encode(&JsonEventCodec, event, id)
    }

    fn decode(&self, event: &RecordedEvent) -> Result<InventoryEvent, EventCodecError> {
        match (event.event_type(), event.schema_version()) {
            ("bicycle-registered", 1) => {
                let v1: BicycleRegisteredV1 = historical(event)?;
                Ok(InventoryEvent::BicycleRegistered(BicycleRegistered {
                    bicycle_id: v1.bicycle_id,
                    condition: if v1.needs_repair {
                        Condition::MaintenanceRequired
                    } else {
                        Condition::Serviceable
                    },
                }))
            }
            ("bicycle-retired", 1) => {
                let v1: BicycleRetiredV1 = historical(event)?;
                Ok(InventoryEvent::BicycleRetired(BicycleRetired {
                    bicycle_id: v1.bicycle_id,
                    reason: RetirementReason::Unrecorded,
                }))
            }
            // Current schemas and unsupported versions: generated strict decoder.
            _ => <JsonEventCodec as EventCodec<Inventory>>::decode(&JsonEventCodec, event),
        }
    }
}

fn historical<T: serde::de::DeserializeOwned>(event: &RecordedEvent) -> Result<T, EventCodecError> {
    serde_json::from_slice(event.payload()).map_err(|error| {
        EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
    })
}

/// The one registry shared by execution, fixtures, dispatch and read models.
pub fn codecs() -> Result<AggregateCodecs, CodecRegistrationError> {
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Inventory, _>(InventoryCodec)?;
    Ok(codecs)
}

pub const INVENTORY_ID: &str = "depot";

/// A current command: it only sees typed, normalized state and raises v2 events.
pub struct RetireBicycle {
    pub bicycle_id: String,
    pub reason: RetirementReason,
}

#[derive(rostfrei::DomainError)]
#[domain(
    id = "unknown-bicycle",
    label = "Unknown bicycle",
    code = "UNKNOWN_BICYCLE",
    message = "Bicycle is not active"
)]
pub struct UnknownBicycle {
    pub bicycle_id: String,
}

pub struct RetireBicycleHandler;

#[async_trait]
impl CommandHandler<RetireBicycle> for RetireBicycleHandler {
    type Rejection = UnknownBicycle;

    async fn handle(
        &self,
        command: &RetireBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<UnknownBicycle> {
        let mut inventory = execution.load::<Inventory>(INVENTORY_ID).await?;
        if !inventory.state().active.contains_key(&command.bicycle_id) {
            return Ok(CommandDecision::Rejected(UnknownBicycle {
                bicycle_id: command.bicycle_id.clone(),
            }));
        }
        inventory.raise(InventoryEvent::BicycleRetired(BicycleRetired {
            bicycle_id: command.bicycle_id.clone(),
            reason: command.reason,
        }));
        Ok(CommandDecision::Accepted)
    }
}

pub type ExampleResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn stream() -> ExampleResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new(Inventory::AGGREGATE_TYPE)?,
        AggregateId::new(INVENTORY_ID)?,
    ))
}

/// Genuine v1 history, as the previous release wrote it.
pub async fn seed_v1_history<S: EventStore>(store: &S) -> ExampleResult<Vec<RecordedEvent>> {
    let operation = OperationId::new("v1-history")?;
    let commit = derive_commit_id(&stream()?, &operation);
    let events = [
        (
            "bicycle-registered",
            r#"{"bicycle_id":"bike-1","needs_repair":false}"#,
        ),
        (
            "bicycle-registered",
            r#"{"bicycle_id":"bike-2","needs_repair":true}"#,
        ),
        (
            "bicycle-registered",
            r#"{"bicycle_id":"bike-3","needs_repair":false}"#,
        ),
        ("bicycle-retired", r#"{"bicycle_id":"bike-3"}"#),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (event_type, payload))| {
        Ok(NewEvent::new(
            derive_event_id(&commit, u32::try_from(ordinal)?),
            event_type,
            1,
            payload.as_bytes().to_vec(),
        )?)
    })
    .collect::<ExampleResult<Vec<_>>>()?;
    let batch = EventBatch::new(
        commit,
        operation,
        ContentFingerprint::digest("v1-history"),
        events,
    )?;
    Ok(store
        .append(&stream()?, ExpectedVersion::NoStream, batch)
        .await?
        .into_events())
}

pub fn metadata(operation: &str) -> ExampleResult<CommandExecutionMetadata> {
    Ok(CommandExecutionMetadata::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(operation),
    )
    .with_bounded_context(BoundedContextName::new(Inventory::BOUNDED_CONTEXT)?))
}
