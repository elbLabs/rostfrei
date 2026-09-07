use rostfrei::{AggregateInstance, DomainService, DomainServiceDefinition};

use super::{
    BicycleTransferRejected, BicycleTransferRejectionReason, BicycleTransferredIn,
    BicycleTransferredOut, TransferBicycle,
};
use crate::domain::{
    BikeRental,
    rental_fleet::{BicycleStatus, RentalFleetAggregate},
};

#[derive(DomainService)]
#[domain(id = "bicycle-transfer", label = "Bicycle transfer")]
pub struct BicycleTransfer;

impl DomainServiceDefinition for BicycleTransfer {
    type Context = BikeRental;
}

impl BicycleTransfer {
    pub fn transfer(
        source: &mut AggregateInstance<RentalFleetAggregate>,
        destination: &mut AggregateInstance<RentalFleetAggregate>,
        command: &TransferBicycle,
    ) -> Result<(), BicycleTransferRejected> {
        let from_fleet_id = source.state().fleet_id().clone();
        let rejection = |reason| BicycleTransferRejected {
            bicycle_id: command.bicycle_id.clone(),
            from_fleet_id: from_fleet_id.clone(),
            to_fleet_id: command.to_fleet_id.clone(),
            reason,
        };

        if from_fleet_id == command.to_fleet_id {
            return Err(rejection(BicycleTransferRejectionReason::SameFleet));
        }
        if destination.state().fleet_id() != &command.to_fleet_id {
            return Err(rejection(
                BicycleTransferRejectionReason::DestinationFleetMismatch,
            ));
        }

        let source_bicycle = source
            .state()
            .bicycles()
            .iter()
            .find(|bicycle| bicycle.bicycle_id() == &command.bicycle_id)
            .ok_or_else(|| rejection(BicycleTransferRejectionReason::SourceBicycleMissing))?;
        if source_bicycle.status() != BicycleStatus::Available {
            return Err(rejection(
                BicycleTransferRejectionReason::SourceBicycleNotAvailable,
            ));
        }
        if destination
            .state()
            .bicycles()
            .iter()
            .any(|bicycle| bicycle.bicycle_id() == &command.bicycle_id)
        {
            return Err(rejection(
                BicycleTransferRejectionReason::DestinationAlreadyContainsBicycle,
            ));
        }

        let condition = source_bicycle.condition();
        let transferred_out = BicycleTransferredOut {
            fleet_id: from_fleet_id.clone(),
            bicycle_id: command.bicycle_id.clone(),
            to_fleet_id: command.to_fleet_id.clone(),
            condition,
        };
        let transferred_in = BicycleTransferredIn {
            fleet_id: command.to_fleet_id.clone(),
            bicycle_id: command.bicycle_id.clone(),
            from_fleet_id,
            condition,
        };

        source.raise(transferred_out);
        destination.raise(transferred_in);
        Ok(())
    }
}
