use rostfrei::{AggregateInstance, domain_action};

use crate::domain::rental_fleet::{BicycleTransferRejected, RentalFleetAggregate, TransferBicycle};

#[domain_action(id = "transfer-bicycle", label = "Transfer bicycle")]
pub trait BicycleTransferAction {
    fn validate_route(
        source: &AggregateInstance<RentalFleetAggregate>,
        command: &TransferBicycle,
    ) -> Result<(), BicycleTransferRejected>;

    fn transfer(
        source: &mut AggregateInstance<RentalFleetAggregate>,
        destination: &mut AggregateInstance<RentalFleetAggregate>,
        command: &TransferBicycle,
    ) -> Result<(), BicycleTransferRejected>;
}
