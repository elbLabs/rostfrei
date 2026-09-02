use rostfrei::domain_model;

use super::{
    BikeRental,
    rental_fleet::{
        Bicycle, BicycleAvailabilityQueries, BicycleCondition, BicycleNotRented, BicycleStatus,
        BicycleUnavailable, RentalFleet, RentalFleetAggregate,
    },
};

pub fn domain_model() -> Result<serde_json::Value, rostfrei::DomainModelError> {
    domain_model! {
        contexts: [BikeRental],
        aggregates: [RentalFleetAggregate],
        entities: [RentalFleet, Bicycle],
        value_objects: [BicycleStatus, BicycleCondition],
        services: [],
        errors: [BicycleUnavailable, BicycleNotRented],
        query_groups: [BicycleAvailabilityQueries],
    }
}
