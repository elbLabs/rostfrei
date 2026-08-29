use std::{convert::Infallible, sync::Arc};

use rostfrei::{
    Aggregate, AggregateInstance, CommandExecutionError, CommandHandler, ContentFingerprint,
    DomainRegistry, EventHistory, EventStore, ExecutionMetadata, Executor, OperationId,
    RegistrationError, StreamAggregateId, StreamAggregateType, StreamId, domain_module,
};
use rostfrei_tracer::{CommandInputField, CommandInputOption, CommandInputOptions, TracerBuilder};
use thiserror::Error;

use crate::rental_fleet::{
    AddBicycle, BicycleCondition, BicycleId, BicycleStatus, ImportRentalFleetInput,
    ImportedBicycle, RentBicycle, RentalFleet, RentalFleetActions, RentalFleetAggregate,
    ReturnBicycle,
};

pub const COMMAND_NAME: &str = "rent-bicycle";
pub const DEMO_FLEET_ID: &str = "city-fleet";

#[derive(Clone, Copy, Debug, Default)]
pub struct RentBicycleInputOptions;

impl CommandInputOptions<RentBicycle> for RentBicycleInputOptions {
    fn fields(&self, state: &RentalFleet) -> Vec<CommandInputField> {
        let bicycles = state
            .bicycles()
            .iter()
            .filter(|bicycle| {
                bicycle.status() == BicycleStatus::Available
                    && bicycle.condition() == BicycleCondition::Serviceable
            })
            .map(|bicycle| {
                CommandInputOption::new(
                    bicycle.bicycle_id().as_str(),
                    bicycle.bicycle_id().as_str(),
                )
                .with_description("Available and serviceable")
            })
            .collect();
        vec![CommandInputField::select("bicycle_id", "Bicycle", bicycles)]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReturnBicycleInputOptions;

impl CommandInputOptions<ReturnBicycle> for ReturnBicycleInputOptions {
    fn fields(&self, state: &RentalFleet) -> Vec<CommandInputField> {
        let bicycles = state
            .bicycles()
            .iter()
            .filter(|bicycle| bicycle.status() == BicycleStatus::Rented)
            .map(|bicycle| {
                CommandInputOption::new(
                    bicycle.bicycle_id().as_str(),
                    bicycle.bicycle_id().as_str(),
                )
                .with_description("Currently rented")
            })
            .collect();
        vec![CommandInputField::select("bicycle_id", "Bicycle", bicycles)]
    }
}

domain_module! {
    pub struct BikeRentalRuntimeModule {
        commands: [RentBicycle, ReturnBicycle, AddBicycle],
    }
}

pub fn tracer_builder(history: Arc<dyn EventHistory>) -> Result<TracerBuilder, RegistrationError> {
    let mut registry = DomainRegistry::new();
    registry.register_module::<BikeRentalRuntimeModule>()?;
    Ok(TracerBuilder::new(history, registry))
}

#[allow(
    clippy::expect_used,
    reason = "compiled and static demo identities are valid by construction"
)]
pub fn demo_stream() -> StreamId {
    StreamId::new(
        StreamAggregateType::new(RentalFleetAggregate::aggregate_type())
            .expect("compiled aggregate type is valid"),
        StreamAggregateId::new(DEMO_FLEET_ID).expect("static aggregate ID is valid"),
    )
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

#[allow(
    clippy::expect_used,
    reason = "static demo operation and bicycle identities are valid by construction"
)]
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
    Executor::new(store.clone())
        .execute::<RentalFleetAggregate, _>(metadata, &command)
        .await?;
    Ok(())
}
