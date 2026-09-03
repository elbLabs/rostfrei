use std::convert::Infallible;

use rostfrei::{
    AggregateInstance, CommandExecutionError, CommandHandler, ContentFingerprint, EventStore,
    ExecutionMetadata, Executor, OperationId, StreamId,
};
use thiserror::Error;

use crate::rental_fleet::{
    self, BicycleCondition, BicycleId, BicycleStatus, ImportRentalFleetAction as _,
    ImportRentalFleetInput, ImportedBicycle, RentalFleetAggregate,
};

pub const DEMO_FLEET_ID: &str = "city-fleet";

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

#[allow(
    clippy::expect_used,
    reason = "compiled and static demo identities are valid by construction"
)]
pub fn demo_stream() -> StreamId {
    rental_fleet::stream_id(DEMO_FLEET_ID).expect("static demo stream identity is valid")
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
