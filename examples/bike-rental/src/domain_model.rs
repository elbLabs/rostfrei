use domain::domain_model;

use crate::rental::{
    Bicycle, BicycleAvailability, BicycleAvailabilityQueries, BicycleCondition, BicycleId,
    BicycleStatus, BicycleUnavailable, BikeRental, FleetId, RentBicycle, RentalEligibility,
    RentalEligibilityInput, RentalFleet, RentalFleetAggregate,
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
        ],
        services: [],
        commands: [RentBicycle],
        errors: [BicycleUnavailable],
        query_groups: [BicycleAvailabilityQueries],
    }
}
