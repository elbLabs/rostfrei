use rostfrei::domain_model;

use super::{
    BikeRental,
    bicycle_transfer::BicycleTransfer,
    rental_fleet::{
        Bicycle, BicycleAlreadyInFleet, BicycleCannotBeRetired, BicycleCondition, BicycleNotRented,
        BicycleTransferRejected, BicycleTransferRejectionReason, BicycleUnavailable,
        RegistrationNumber, RentalFleet, RentalFleetAggregate,
    },
};

pub fn domain_model() -> Result<serde_json::Value, rostfrei::DomainModelError> {
    domain_model! {
        contexts: [BikeRental],
        aggregates: [RentalFleetAggregate],
        entities: [RentalFleet, Bicycle],
        value_objects: [BicycleCondition, RegistrationNumber, BicycleTransferRejectionReason],
        services: [BicycleTransfer],
        errors: [
            BicycleUnavailable,
            BicycleNotRented,
            BicycleAlreadyInFleet,
            BicycleCannotBeRetired,
            BicycleTransferRejected,
        ],
    }
}
