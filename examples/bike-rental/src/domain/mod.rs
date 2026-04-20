use rostfrei::domain_model;

mod bike_rental;

pub use bike_rental::{BikeRental, rental_fleet};

use rental_fleet::{
    AddBicycle, Bicycle, BicycleAvailability, BicycleAvailabilityQueries, BicycleCondition,
    BicycleId, BicycleNotRented, BicycleStatus, BicycleUnavailable, FleetId,
    ImportRentalFleetInput, ImportedBicycle, RentBicycle, RentalFleet, RentalFleetAggregate,
    ReturnBicycle,
};

pub fn domain_model() -> Result<serde_json::Value, rostfrei::DomainModelError> {
    domain_model! {
        contexts: [BikeRental],
        aggregates: [RentalFleetAggregate],
        entities: [RentalFleet, Bicycle],
        identities: [FleetId, BicycleId],
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
