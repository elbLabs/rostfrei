use rostfrei::domain_model;

use super::{
    BikeRental,
    rental_fleet::{
        AddBicycle, Bicycle, BicycleAvailability, BicycleAvailabilityQueries, BicycleCondition,
        BicycleNotRented, BicycleStatus, BicycleUnavailable, ImportRentalFleetInput,
        ImportedBicycle, RentBicycle, RentalFleet, RentalFleetAggregate, ReturnBicycle,
    },
};

pub fn domain_model() -> Result<serde_json::Value, rostfrei::DomainModelError> {
    domain_model! {
        contexts: [BikeRental],
        aggregates: [RentalFleetAggregate],
        entities: [RentalFleet, Bicycle],
        value_objects: [
            BicycleStatus,
            BicycleCondition,
            BicycleAvailability,
            ImportRentalFleetInput,
            ImportedBicycle,
        ],
        services: [],
        commands: [RentBicycle, ReturnBicycle, AddBicycle],
        errors: [BicycleUnavailable, BicycleNotRented],
        query_groups: [BicycleAvailabilityQueries],
    }
}
