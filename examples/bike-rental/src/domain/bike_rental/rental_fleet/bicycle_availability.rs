use rostfrei::{ValueObject, domain_queries};

use super::assess_rental_eligibility::RentalEligibilityOutcome;
use super::{BicycleId, RentalFleet, RentalFleetAggregate};

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-availability",
    label = "Bicycle availability",
    owner = RentalFleetAggregate
)]
pub enum BicycleAvailability {
    Available,
    Unavailable,
}

#[domain_queries(group = BicycleAvailabilityQueries)]
impl RentalFleetAggregate {
    #[query(id = "bicycle-availability", label = "Bicycle availability")]
    pub fn bicycle_availability(
        root: &RentalFleet,
        input: &BicycleId,
    ) -> Option<BicycleAvailability> {
        root.bicycles
            .iter()
            .find(|bicycle| bicycle.bicycle_id() == input)
            .map(|bicycle| {
                match Self::assess_rental_eligibility(bicycle.status(), bicycle.condition()) {
                    RentalEligibilityOutcome::Eligible => BicycleAvailability::Available,
                    RentalEligibilityOutcome::AlreadyRented
                    | RentalEligibilityOutcome::MaintenanceRequired => {
                        BicycleAvailability::Unavailable
                    }
                }
            })
    }
}
