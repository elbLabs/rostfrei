use rostfrei::domain_model;

use crate::rental::{
    Bicycle, BicycleAvailability, BicycleAvailabilityQueries, BicycleCondition, BicycleId,
    BicycleStatus, BicycleUnavailable, BikeRental, FleetId, ImportRentalFleetInput,
    ImportedBicycle, RentBicycle, RentalDenialReason, RentalFleet, RentalFleetAggregate,
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
            RentalDenialReason,
            ImportedBicycle,
            ImportRentalFleetInput,
        ],
        services: [],
        commands: [RentBicycle],
        errors: [BicycleUnavailable],
        query_groups: [BicycleAvailabilityQueries],
    }
}
