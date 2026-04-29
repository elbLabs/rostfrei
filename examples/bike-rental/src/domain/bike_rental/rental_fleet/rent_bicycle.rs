use rostfrei::{
    AggregateInstance, Apply, Command, CommandHandler, CommandType, DomainError, DomainEvent,
};
use serde::{Deserialize, Serialize};

use super::BicycleStatus;
use super::bicycle::BicycleStatusActions;
use super::{BicycleId, FleetId, RentalFleet, RentalFleetActions, RentalFleetAggregate};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "rent-bicycle",
    label = "Rent bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleUnavailable,
    json,
    runtime
)]
pub struct RentBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-unavailable",
    label = "Bicycle unavailable",
    owner = RentalFleetAggregate,
    code = "BICYCLE_UNAVAILABLE",
    message = "The requested bicycle cannot currently be rented.",
    json
)]
pub struct BicycleUnavailable {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = <RentBicycle as CommandType>::Rejection;

    fn handle(
        command: &RentBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.rent_bicycle(command.bicycle_id.clone())
    }
}

impl Apply<BicycleRented> for RentalFleet {
    fn apply(&mut self, event: &BicycleRented) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.mark_rented(BicycleStatus::Rented);
        }
    }
}
