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

impl Initialize<RentalFleetAggregate> for RentalFleet {
    fn initialize(stream_id: &StreamId) -> Self {
        Self::new(
            FleetId::new(stream_id.aggregate_id().as_str())
                .expect("a valid stream aggregate ID is a valid fleet ID"),
            Vec::new(),
        )
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

pub fn demo_stream() -> StreamId {
    StreamId::new(
        StreamAggregateType::new(RentalFleetAggregate::aggregate_type())
            .expect("compiled aggregate type is valid"),
        StreamAggregateId::new(DEMO_FLEET_ID).expect("static aggregate ID is valid"),
    )
}

pub fn rental_fleet_stream(aggregate_id: &str) -> Result<StreamId, &'static str> {
    let aggregate_type = StreamAggregateType::new(RentalFleetAggregate::aggregate_type())
        .map_err(|_| "invalid rental fleet aggregate type")?;
    let aggregate_id =
        StreamAggregateId::new(aggregate_id).map_err(|_| "invalid rental fleet ID")?;
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

#[derive(Debug, Error)]
pub enum SeedError {
    #[error(transparent)]
    Execution(#[from] CommandExecutionError),
}

pub async fn seed_demo<S>(store: &S) -> Result<(), SeedError>
where
    S: EventStore + Clone,
{
    let stream = demo_stream();
    let metadata = ExecutionMetadata::new(
        stream,
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
    let outcome = Executor::new(store.clone())
        .execute::<RentalFleetAggregate, _>(metadata, &command)
        .await?;
    match outcome {
        CommandOutcome::Accepted(_) => Ok(()),
        CommandOutcome::Rejected(rejection) => match rejection {},
    }
}
