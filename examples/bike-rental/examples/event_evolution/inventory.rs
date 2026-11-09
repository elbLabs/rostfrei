//! Private-event schema evolution: current v2 events, explicit v1 readers.
//!
//! v1 `bicycle-registered` stored `needs_repair: bool`; v2 stores a condition.
//! The conversion preserves meaning. v1 `bicycle-retired` recorded no reason, so
//! it becomes the explicit `RetirementReason::Unrecorded` instead of a guess.

use std::collections::BTreeMap;

use async_trait::async_trait;
use rostfrei::core::{
    AggregateId, AggregateType, FixtureEventStore, derive_commit_id, derive_event_id,
};
use rostfrei::{
    Aggregate, AggregateCodecs, BoundedContextName, CommandDecision, CommandExecution,
    CommandExecutionMetadata, CommandHandler, CommandHandlingResult, ContentFingerprint,
    EventBatch, EventCodecError, EventCodecErrorKind, EventId, ExpectedVersion, NewEvent,
    OperationId, RecordedEvent, StreamId,
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

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(
    id = "bicycle-registered",
    label = "Bicycle registered",
    schema_version = 1
)]
struct BicycleRegisteredV1 {
    bicycle_id: String,
    needs_repair: bool,
}

#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "bicycle-retired", label = "Bicycle retired", schema_version = 1)]
struct BicycleRetiredV1 {
    bicycle_id: String,
}

/// Writes the generated current format and reads every supported historical one.
pub struct InventoryCodec;

#[rostfrei::event_codec(aggregate = Inventory)]
impl InventoryCodec {
    #[upcast]
    fn registered_v1(old: BicycleRegisteredV1) -> BicycleRegistered {
        BicycleRegistered {
            bicycle_id: old.bicycle_id,
            condition: if old.needs_repair {
                Condition::MaintenanceRequired
            } else {
                Condition::Serviceable
            },
        }
    }

    #[upcast]
    fn retired_v1(old: BicycleRetiredV1) -> BicycleRetired {
        BicycleRetired {
            bicycle_id: old.bicycle_id,
            reason: RetirementReason::Unrecorded,
        }
    }
}

/// The one registry shared by execution, fixtures, dispatch and read models.
pub fn codecs() -> ExampleResult<AggregateCodecs> {
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Inventory, _>(InventoryCodec::builder().build()?)?;
    Ok(codecs)
}

/// First rollout: dual readers, but the original v1 writer until all readers upgrade.
pub fn compatibility_codecs() -> ExampleResult<AggregateCodecs> {
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Inventory, _>(InventoryCodec::builder().with_encoder(encode_v1).build()?)?;
    Ok(codecs)
}

/// Application-owned encoding: never silently discard information to fit v1.
fn encode_v1(event: &InventoryEvent, id: EventId) -> Result<NewEvent, EventCodecError> {
    match event {
        InventoryEvent::BicycleRegistered(event) => old_json(
            &BicycleRegisteredV1 {
                bicycle_id: event.bicycle_id.clone(),
                needs_repair: match event.condition {
                    Condition::Serviceable => false,
                    Condition::MaintenanceRequired => true,
                },
            },
            id,
        ),
        InventoryEvent::BicycleRetired(event) if event.reason == RetirementReason::Unrecorded => {
            old_json(
                &BicycleRetiredV1 {
                    bicycle_id: event.bicycle_id.clone(),
                },
                id,
            )
        }
        InventoryEvent::BicycleRetired(_) => Err(EventCodecError::new(
            EventCodecErrorKind::EncodingFailed,
            "bicycle-retired v1 cannot preserve a retirement reason; upgrade readers before enabling v2 writes",
        )),
    }
}

fn old_json<E: rostfrei::DomainEvent + Serialize>(
    event: &E,
    id: EventId,
) -> Result<NewEvent, EventCodecError> {
    let payload = serde_json::to_vec(event).map_err(|error| {
        EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
    })?;
    NewEvent::new(id, E::LOCAL_ID, E::SCHEMA_VERSION, payload).map_err(|error| {
        EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
    })
}

pub const INVENTORY_ID: &str = "depot";

/// A current command: it only sees typed, normalized state and raises v2 events.
#[derive(rostfrei::Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "retire-bicycle", label = "Retire bicycle", context = FleetEvolution)]
pub struct RetireBicycle {
    pub bicycle_id: String,
    pub reason: RetirementReason,
}

#[derive(rostfrei::BoundedContext)]
#[domain(id = "fleet-evolution", label = "Fleet evolution")]
pub struct FleetEvolution;

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

/// A fixture using genuine v1 payloads, as the previous release wrote them.
pub async fn seed_v1_history<S: FixtureEventStore>(store: &S) -> ExampleResult<Vec<RecordedEvent>> {
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
        .append_fixture(&stream()?, ExpectedVersion::NoStream, batch)
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
