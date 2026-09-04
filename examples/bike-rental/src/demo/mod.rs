use std::convert::Infallible;

use rostfrei::{
    AggregateInstance, CommandExecutionError, CommandHandler, CommitId, ContentFingerprint,
    EventBatch, EventId, EventStore, ExecutionMetadata, Executor, ExpectedVersion, NewEvent,
    OperationId, StreamAggregateId, StreamAggregateType, StreamId,
};
use rostfrei_tracer::{MaterializedTestFixture, TestFixture, TestFixtureEvent, TestFixtureStream};
use serde_json::json;
use thiserror::Error;

use crate::rental_fleet::{
    self, BicycleCondition, BicycleId, BicycleStatus, ImportRentalFleetInput, ImportedBicycle,
    RentalFleetActions, RentalFleetAggregate,
};

pub const DEMO_FLEET_ID: &str = "city-fleet";

pub const AVAILABLE_FLEET_FIXTURE: &str = "available-fleet";
pub const RENTED_FLEET_FIXTURE: &str = "rented-fleet";

pub fn available_fleet_fixture() -> TestFixture {
    fleet_fixture(AVAILABLE_FLEET_FIXTURE, false)
}

pub fn rented_fleet_fixture() -> TestFixture {
    fleet_fixture(RENTED_FLEET_FIXTURE, true)
}

fn fleet_fixture(name: &str, rented: bool) -> TestFixture {
    let revision = ContentFingerprint::digest(if rented {
        "bike-rental-rented-fleet-v1"
    } else {
        "bike-rental-available-fleet-v1"
    })
    .to_string();
    let mut events = vec![TestFixtureEvent {
        event_type: "rental-fleet-imported".to_owned(),
        schema_version: 1,
        stream_version: 1,
        payload: json!({
            "fleet_id": DEMO_FLEET_ID,
            "bicycles": [
                {
                    "bicycle_id": "bike-42",
                    "status": "available",
                    "condition": "serviceable"
                },
                {
                    "bicycle_id": "bike-99",
                    "status": "available",
                    "condition": "maintenance-required"
                }
            ]
        }),
    }];
    if rented {
        events.push(TestFixtureEvent {
            event_type: "bicycle-rented".to_owned(),
            schema_version: 1,
            stream_version: 2,
            payload: json!({
                "fleet_id": DEMO_FLEET_ID,
                "bicycle_id": "bike-42"
            }),
        });
    }
    TestFixture {
        name: name.to_owned(),
        revision,
        streams: vec![TestFixtureStream {
            aggregate_type: "bike-rental/rental-fleet".to_owned(),
            aggregate_id: DEMO_FLEET_ID.to_owned(),
            events,
        }],
    }
}

struct ImportDemoFleet {
    bicycles: Vec<ImportedBicycle>,
}

impl CommandHandler<ImportDemoFleet> for RentalFleetAggregate {
    type Rejection = Infallible;

    fn handle(
        command: &ImportDemoFleet,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.import_rental_fleet(ImportRentalFleetInput::new(command.bicycles.clone()));
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SeedError {
    #[error(transparent)]
    Execution(#[from] CommandExecutionError),
    #[error("invalid declarative fixture: {0}")]
    Fixture(String),
}

#[allow(
    clippy::expect_used,
    reason = "compiled and static demo identities are valid by construction"
)]
pub fn demo_stream() -> StreamId {
    rental_fleet::stream_id(DEMO_FLEET_ID).expect("static demo stream identity is valid")
}

pub async fn materialize_fixture<S>(
    store: &S,
    fixture: &MaterializedTestFixture,
) -> Result<(), SeedError>
where
    S: EventStore,
{
    for stream in &fixture.streams {
        let stream_id = StreamId::new(
            StreamAggregateType::new(&stream.aggregate_type)
                .map_err(|error| SeedError::Fixture(error.to_string()))?,
            StreamAggregateId::new(&stream.aggregate_id)
                .map_err(|error| SeedError::Fixture(error.to_string()))?,
        );
        let events = stream
            .events
            .iter()
            .map(|event| {
                let payload = event
                    .payload
                    .as_ref()
                    .ok_or_else(|| SeedError::Fixture("fixture payload was redacted".to_owned()))?;
                NewEvent::new(
                    EventId::new(&event.event_id)
                        .map_err(|error| SeedError::Fixture(error.to_string()))?,
                    &event.event_type,
                    event.schema_version,
                    serde_json::to_vec(payload)
                        .map_err(|error| SeedError::Fixture(error.to_string()))?,
                )
                .map_err(|error| SeedError::Fixture(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let batch = EventBatch::new(
            CommitId::new(&stream.commit_id)
                .map_err(|error| SeedError::Fixture(error.to_string()))?,
            OperationId::new(&stream.operation_id)
                .map_err(|error| SeedError::Fixture(error.to_string()))?,
            ContentFingerprint::from_hex(&stream.operation_fingerprint)
                .map_err(|error| SeedError::Fixture(error.to_string()))?,
            events,
        )
        .map_err(|error| SeedError::Fixture(error.to_string()))?;
        store
            .append(&stream_id, ExpectedVersion::NoStream, batch)
            .await
            .map_err(|error| SeedError::Fixture(error.to_string()))?;
    }
    Ok(())
}

#[allow(
    clippy::expect_used,
    reason = "static demo operation and bicycle identities are valid by construction"
)]
pub async fn seed_demo<S>(store: &S) -> Result<(), SeedError>
where
    S: EventStore + Clone,
{
    let metadata = ExecutionMetadata::new(
        demo_stream(),
        OperationId::new("seed-city-fleet").expect("static operation ID is valid"),
        ContentFingerprint::digest("bike-rental-demo-v1"),
    );
    let command = ImportDemoFleet {
        bicycles: vec![
            ImportedBicycle {
                bicycle_id: BicycleId::new("bike-42").expect("static bicycle ID is valid"),
                status: BicycleStatus::Available,
                condition: BicycleCondition::Serviceable,
            },
            ImportedBicycle {
                bicycle_id: BicycleId::new("bike-99").expect("static bicycle ID is valid"),
                status: BicycleStatus::Available,
                condition: BicycleCondition::MaintenanceRequired,
            },
        ],
    };
    Executor::new(store.clone())
        .execute::<RentalFleetAggregate, _>(metadata, &command)
        .await?;
    Ok(())
}
