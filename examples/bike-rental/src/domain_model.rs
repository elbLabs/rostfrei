use rostfrei::domain_model;

use crate::rental::{
    AddBicycle, Bicycle, BicycleAvailability, BicycleAvailabilityQueries, BicycleCondition,
    BicycleId, BicycleNotRented, BicycleStatus, BicycleUnavailable, BikeRental, FleetId,
    ImportedBicycle, RentBicycle, RentalDenialReason, RentalFleet, RentalFleetAggregate,
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
            ImportedBicycle,
            ImportRentalFleetInput,
        ],
        services: [],
        commands: [RentBicycle, ReturnBicycle, AddBicycle],
        errors: [BicycleUnavailable, BicycleNotRented],
        query_groups: [BicycleAvailabilityQueries],
    }
}
