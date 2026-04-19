use std::{convert::Infallible, sync::Arc};

use rostfrei::{
    Aggregate, AggregateInstance, Apply, CommandExecutionError, CommandHandler, CommandOutcome,
    ContentFingerprint, EventHistory, EventStore, ExecutionMetadata, Executor, Initialize,
    OperationId, StreamAggregateId, StreamAggregateType, StreamId,
};
use rostfrei_control_plane::ControlPlaneBuilder;
use thiserror::Error;

use crate::rental::{
    Bicycle, BicycleCondition, BicycleId, BicycleRented, BicycleStatus, FleetId,
    ImportRentalFleetInput, ImportedBicycle, RentBicycle, RentalFleet, RentalFleetActions,
    RentalFleetAggregate, RentalFleetImported,
};

pub const DEMO_FLEET_ID: &str = "city-fleet";
const DEMO_SEED_OPERATION_ID: &str = "seed-city-fleet";
const DEMO_BICYCLE_IDS: [&str; 2] = ["bike-42", "bike-99"];

impl Initialize<RentalFleetAggregate> for RentalFleet {
    fn initialize(stream_id: &StreamId) -> Self {
        Self::new(FleetId::from(stream_id.aggregate_id()), Vec::new())
    }
}

impl Apply<RentalFleetImported> for RentalFleet {
    fn apply(&mut self, event: &RentalFleetImported) {
        *self = Self::new(
            self.fleet_id().clone(),
            event
                .bicycles
                .iter()
                .map(|bicycle| {
                    Bicycle::new(
                        bicycle.bicycle_id.clone(),
                        bicycle.status,
                        bicycle.condition,
                    )
                })
                .collect(),
        );
    }
}

impl Apply<BicycleRented> for RentalFleet {
    fn apply(&mut self, event: &BicycleRented) {
        self.apply_rental(&event.bicycle_id);
    }
}

impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = <RentBicycle as rostfrei::DomainCommandType>::Rejection;

    fn handle(
        command: &RentBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.rent_bicycle(command.bicycle_id.clone())
    }
}

pub fn control_plane_builder(history: Arc<dyn EventHistory>) -> ControlPlaneBuilder {
    ControlPlaneBuilder::new(history)
}

pub fn demo_stream() -> Result<StreamId, DemoFixtureError> {
    let aggregate_type = StreamAggregateType::new(RentalFleetAggregate::aggregate_type())
        .map_err(|error| DemoFixtureError::invalid_identity("aggregate type", error.to_string()))?;
    let aggregate_id = StreamAggregateId::new(DEMO_FLEET_ID)
        .map_err(|error| DemoFixtureError::invalid_identity("aggregate ID", error.to_string()))?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
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

#[derive(Debug, Eq, Error, PartialEq)]
pub enum DemoFixtureError {
    #[error("invalid demo {identity}: {message}")]
    InvalidIdentity {
        identity: &'static str,
        message: String,
    },
}

impl DemoFixtureError {
    fn invalid_identity(identity: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidIdentity {
            identity,
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SeedError {
    #[error(transparent)]
    Fixture(#[from] DemoFixtureError),
    #[error(transparent)]
    Execution(#[from] CommandExecutionError),
}

pub async fn seed_demo<S>(store: &S) -> Result<(), SeedError>
where
    S: EventStore + Clone,
{
    let stream = demo_stream()?;
    let operation_id = OperationId::new(DEMO_SEED_OPERATION_ID)
        .map_err(|error| DemoFixtureError::invalid_identity("operation ID", error.to_string()))?;
    let metadata = ExecutionMetadata::new(
        stream,
        operation_id,
        ContentFingerprint::digest("bike-rental-demo-v1"),
    );
    let command = ImportDemoFleet {
        bicycles: demo_bicycles()?,
    };
    let outcome = Executor::new(store.clone())
        .execute::<RentalFleetAggregate, _>(metadata, &command)
        .await?;
    match outcome {
        CommandOutcome::Accepted(_) => Ok(()),
        CommandOutcome::Rejected(rejection) => match rejection {},
    }
}

fn demo_bicycles() -> Result<Vec<ImportedBicycle>, DemoFixtureError> {
    let [serviceable_id, maintenance_id] = DEMO_BICYCLE_IDS;
    let bicycle_id = |value| {
        BicycleId::new(value).ok_or_else(|| {
            DemoFixtureError::invalid_identity(
                "bicycle ID",
                format!("`{value}` must be non-empty and trimmed"),
            )
        })
    };
    Ok(vec![
        ImportedBicycle {
            bicycle_id: bicycle_id(serviceable_id)?,
            status: BicycleStatus::Available,
            condition: BicycleCondition::Serviceable,
        },
        ImportedBicycle {
            bicycle_id: bicycle_id(maintenance_id)?,
            status: BicycleStatus::Available,
            condition: BicycleCondition::MaintenanceRequired,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_fixture_retains_its_contract_ids() {
        let fixture_ids = demo_stream().and_then(|stream| {
            demo_bicycles().map(|bicycles| {
                let bicycle_ids = bicycles
                    .iter()
                    .map(|bicycle| bicycle.bicycle_id.as_str().to_owned())
                    .collect::<Vec<_>>();
                (
                    stream.aggregate_type().as_str().to_owned(),
                    stream.aggregate_id().as_str().to_owned(),
                    bicycle_ids,
                )
            })
        });

        assert_eq!(
            fixture_ids,
            Ok((
                "bike-rental/rental-fleet".to_owned(),
                DEMO_FLEET_ID.to_owned(),
                DEMO_BICYCLE_IDS.map(str::to_owned).to_vec(),
            ))
        );
    }
}
