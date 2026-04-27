use rostfrei::{
    AggregateInstance, Apply, Command, CommandHandler, CommandType, DomainError, DomainEvent,
};
use serde::{Deserialize, Serialize};

use super::bicycle::BicycleStatusActions;
use super::{BicycleId, FleetId, RentalFleet, RentalFleetActions, RentalFleetAggregate};
use super::{BicycleStatus, assess_rental_eligibility::RentalEligibilityOutcome};

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

pub(super) fn rent_bicycle(
    aggregate: &mut AggregateInstance<RentalFleetAggregate>,
    input: &BicycleId,
) -> Result<(), BicycleUnavailable> {
    let event = {
        let root = aggregate.state();
        let bicycle = root
            .bicycles
            .iter()
            .find(|bicycle| bicycle.bicycle_id() == input)
            .ok_or_else(|| BicycleUnavailable {
                bicycle_id: input.clone(),
            })?;
        match RentalFleetAggregate::assess_rental_eligibility(bicycle.status(), bicycle.condition())
        {
            RentalEligibilityOutcome::Eligible => BicycleRented {
                fleet_id: root.fleet_id.clone(),
                bicycle_id: input.clone(),
            },
            RentalEligibilityOutcome::AlreadyRented
            | RentalEligibilityOutcome::MaintenanceRequired => {
                return Err(BicycleUnavailable {
                    bicycle_id: input.clone(),
                });
            }
        }
    };

    aggregate.raise(event);
    Ok(())
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
