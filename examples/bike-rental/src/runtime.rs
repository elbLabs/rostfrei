use std::{convert::Infallible, sync::Arc};

use rostfrei::{
    Aggregate, AggregateInstance, Apply, CommandHandler, ContentFingerprint, DomainRegistry,
    EventHistory, EventStore, ExecutionError, ExecutionMetadata, Executor, Initialize, OperationId,
    RegistrationError, StreamAggregateId, StreamAggregateType, StreamId, domain_module,
};
use rostfrei_control_plane::{CommandWireCodec, CommandWireCodecError, ControlPlaneBuilder};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::rental::{
    Bicycle, BicycleCondition, BicycleId, BicycleRented, BicycleStatus, BicycleUnavailable,
    FleetId, ImportedBicycle, RentBicycle, RentalFleet, RentalFleetAggregate, RentalFleetImported,
    decide_rental,
};

pub const COMMAND_NAME: &str = "bike-rental.rent-bicycle";
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
    type Rejection = BicycleUnavailable;

    fn handle(
        command: &RentBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        let event = decide_rental(aggregate.state(), command)?;
        aggregate.raise(event);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RentBicycleWireCodec;

impl CommandWireCodec<RentBicycle> for RentBicycleWireCodec {
    fn decode(&self, payload: &Value) -> Result<RentBicycle, CommandWireCodecError> {
        #[derive(Deserialize)]
        struct Wire {
            bicycle_id: String,
        }

        let wire: Wire = serde_json::from_value(payload.clone())
            .map_err(|error| CommandWireCodecError::new(error.to_string()))?;
        let bicycle_id = BicycleId::new(wire.bicycle_id).ok_or_else(|| {
            CommandWireCodecError::new("bicycle_id must be non-empty and trimmed")
        })?;
        Ok(RentBicycle { bicycle_id })
    }

    fn encode_rejection(
        &self,
        rejection: &BicycleUnavailable,
    ) -> Result<Value, CommandWireCodecError> {
        Ok(json!({
            "code": "BICYCLE_UNAVAILABLE",
            "message": "The requested bicycle cannot currently be rented.",
            "bicycle_id": rejection.bicycle_id.as_str(),
        }))
    }
}

domain_module! {
    pub struct BikeRentalRuntimeModule {
        name: "bike-rental",
        commands: [
            RentBicycle => {
                name: "bike-rental.rent-bicycle",
                version: 1,
            },
        ],
    }
}

pub fn control_plane_builder(
    history: Arc<dyn EventHistory>,
) -> Result<ControlPlaneBuilder, RegistrationError> {
    let mut registry = DomainRegistry::new();
    registry.register_module::<BikeRentalRuntimeModule>()?;
    Ok(ControlPlaneBuilder::new(history, registry))
}

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
        aggregate.raise(RentalFleetImported {
            fleet_id: aggregate.state().fleet_id().clone(),
            bicycles: command.bicycles.clone(),
        });
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SeedError {
    #[error(transparent)]
    Execution(#[from] ExecutionError<Infallible>),
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
    Executor::new(store.clone())
        .execute::<RentalFleetAggregate, _>(metadata, &command)
        .await?;
    Ok(())
}
