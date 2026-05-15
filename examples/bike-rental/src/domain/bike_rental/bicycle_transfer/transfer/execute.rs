use rostfrei::AggregateInstance;

use super::BicycleTransferAction;
use crate::domain::{
    bicycle_transfer::BicycleTransfer,
    rental_fleet::{
        BicycleStatus, BicycleTransferRejected, BicycleTransferRejectionReason,
        RentalFleetAggregate, TransferBicycle, TransferBicycleInAction as _,
        TransferBicycleOutAction as _,
    },
};

impl BicycleTransferAction for BicycleTransfer {
    fn validate_route(
        source: &AggregateInstance<RentalFleetAggregate>,
        command: &TransferBicycle,
    ) -> Result<(), BicycleTransferRejected> {
        if source.state().fleet_id() == &command.to_fleet_id {
            return Err(rejection(
                source,
                command,
                BicycleTransferRejectionReason::SameFleet,
            ));
        }
        Ok(())
    }

    fn transfer(
        source: &mut AggregateInstance<RentalFleetAggregate>,
        destination: &mut AggregateInstance<RentalFleetAggregate>,
        command: &TransferBicycle,
    ) -> Result<(), BicycleTransferRejected> {
        Self::validate_route(source, command)?;
        let from_fleet_id = source.state().fleet_id().clone();
        let rejected = |reason| rejection(source, command, reason);

        if destination.state().fleet_id() != &command.to_fleet_id {
            return Err(rejected(
                BicycleTransferRejectionReason::DestinationFleetMismatch,
            ));
        }

        let source_bicycle = source
            .state()
            .bicycles()
            .iter()
            .find(|bicycle| bicycle.bicycle_id() == &command.bicycle_id)
            .ok_or_else(|| rejected(BicycleTransferRejectionReason::SourceBicycleMissing))?;
        if source_bicycle.status() != BicycleStatus::Available {
            return Err(rejected(
                BicycleTransferRejectionReason::SourceBicycleNotAvailable,
            ));
        }
        if destination
            .state()
            .bicycles()
            .iter()
            .any(|bicycle| bicycle.bicycle_id() == &command.bicycle_id)
        {
            return Err(rejected(
                BicycleTransferRejectionReason::DestinationAlreadyContainsBicycle,
            ));
        }

        let condition = source_bicycle.condition();
        source.transfer_bicycle_out(
            command.bicycle_id.clone(),
            command.to_fleet_id.clone(),
            condition,
        );
        destination.transfer_bicycle_in(command.bicycle_id.clone(), from_fleet_id, condition);
        Ok(())
    }
}

fn rejection(
    source: &AggregateInstance<RentalFleetAggregate>,
    command: &TransferBicycle,
    reason: BicycleTransferRejectionReason,
) -> BicycleTransferRejected {
    BicycleTransferRejected {
        bicycle_id: command.bicycle_id.clone(),
        from_fleet_id: source.state().fleet_id().clone(),
        to_fleet_id: command.to_fleet_id.clone(),
        reason,
    }
}
