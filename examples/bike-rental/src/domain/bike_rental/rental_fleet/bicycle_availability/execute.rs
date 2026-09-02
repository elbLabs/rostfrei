use super::{BicycleAvailability, BicycleAvailabilityQuery};
use crate::domain::bike_rental::rental_fleet::{
    BicycleId, RentalFleet, RentalFleetAggregate,
    assess_rental_eligibility::RentalEligibilityOutcome,
};

impl BicycleAvailabilityQuery for RentalFleet {
    fn bicycle_availability(&self, input: &BicycleId) -> Option<BicycleAvailability> {
        self.bicycles()
            .iter()
            .find(|bicycle| bicycle.bicycle_id() == input)
            .map(|bicycle| {
                match RentalFleetAggregate::assess_rental_eligibility(
                    bicycle.status(),
                    bicycle.condition(),
                ) {
                    RentalEligibilityOutcome::Eligible => BicycleAvailability::Available,
                    RentalEligibilityOutcome::AlreadyRented
                    | RentalEligibilityOutcome::MaintenanceRequired => {
                        BicycleAvailability::Unavailable
                    }
                }
            })
    }
}
