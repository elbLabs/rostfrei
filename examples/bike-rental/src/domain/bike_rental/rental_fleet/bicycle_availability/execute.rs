use super::{BicycleAvailability, BicycleAvailabilityQuery};
use crate::domain::bike_rental::rental_fleet::{
    BicycleId, RentalFleet,
    assess_rental_eligibility::{RentalEligibilityOutcome, RentalEligibilityPolicy as _},
};

impl BicycleAvailabilityQuery for RentalFleet {
    fn bicycle_availability(&self, input: &BicycleId) -> Option<BicycleAvailability> {
        self.bicycles()
            .iter()
            .find(|bicycle| bicycle.bicycle_id() == input)
            .map(|bicycle| match bicycle.assess_rental_eligibility() {
                RentalEligibilityOutcome::Eligible => BicycleAvailability::Available,
                RentalEligibilityOutcome::UnavailableStatus
                | RentalEligibilityOutcome::MaintenanceRequired => BicycleAvailability::Unavailable,
            })
    }
}
