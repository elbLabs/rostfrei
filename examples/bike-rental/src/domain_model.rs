use rostfrei::domain_model;

use crate::rental::{
    Bicycle, BicycleAvailability, BicycleAvailabilityQueries, BicycleCondition, BicycleId,
    BicycleStatus, BicycleUnavailable, BikeRental, FleetId, ImportedBicycle, RentBicycle,
    RentalEligibility, RentalEligibilityInput, RentalFleet, RentalFleetAggregate,
};

pub fn domain_model() -> serde_json::Value {
    domain_model! {
        contexts: [BikeRental],
        aggregates: [RentalFleetAggregate],
        entities: [RentalFleet, Bicycle],
        identities: [FleetId, BicycleId],
        value_objects: [
            BicycleStatus,
            BicycleCondition,
            BicycleAvailability,
            RentalEligibilityInput,
            RentalEligibility,
            ImportedBicycle,
        ],
        services: [],
        commands: [RentBicycle],
        errors: [BicycleUnavailable],
        query_groups: [BicycleAvailabilityQueries],
    }
}
