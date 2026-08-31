use rostfrei::{
    AggregateInstance, Apply, Command, CommandHandler, CommandType, DomainError, DomainEvent,
};
use serde::{Deserialize, Serialize};

use super::{BicycleId, BicycleStatus, FleetId, RentalFleet, RentalFleetAggregate};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "return-bicycle",
    label = "Return bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleNotRented,
    json,
    runtime
)]
pub struct ReturnBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-returned", label = "Bicycle returned")]
pub struct BicycleReturned {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-not-rented",
    label = "Bicycle not rented",
    owner = RentalFleetAggregate,
    code = "BICYCLE_NOT_RENTED",
    message = "The requested bicycle is not currently rented.",
    json
)]
pub struct BicycleNotRented {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

pub(super) fn return_bicycle(
    aggregate: &mut AggregateInstance<RentalFleetAggregate>,
    input: &BicycleId,
) -> Result<(), BicycleNotRented> {
    let root = aggregate.state();
    let rented = root
        .bicycles
        .iter()
        .any(|bicycle| bicycle.bicycle_id() == input && bicycle.status() == BicycleStatus::Rented);
    if !rented {
        return Err(BicycleNotRented {
            bicycle_id: input.clone(),
        });
    }
    let fleet_id = root.fleet_id.clone();
    aggregate.raise(BicycleReturned {
        fleet_id,
        bicycle_id: input.clone(),
    });
    Ok(())
}

impl CommandHandler<ReturnBicycle> for RentalFleetAggregate {
    type Rejection = <ReturnBicycle as CommandType>::Rejection;

    fn handle(
        command: &ReturnBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        return_bicycle(aggregate, &command.bicycle_id)
    }
}

impl Apply<BicycleReturned> for RentalFleet {
    fn apply(&mut self, event: &BicycleReturned) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.mark_available();
        }
    }
}
