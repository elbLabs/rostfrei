use rostfrei::domain_model;

use super::{
    BikeRental,
    rental_fleet::{
        Bicycle, BicycleCondition, BicycleNotRented, BicycleUnavailable, RegistrationNumber,
        RentalFleet, RentalFleetAggregate,
    },
};

pub fn domain_model() -> Result<serde_json::Value, rostfrei::DomainModelError> {
    domain_model! {
        contexts: [BikeRental],
        aggregates: [RentalFleetAggregate],
        entities: [RentalFleet, Bicycle],
        value_objects: [BicycleCondition, RegistrationNumber],
        services: [],
        errors: [BicycleUnavailable, BicycleNotRented],
    }
}
