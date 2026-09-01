use rostfrei::domain_queries;

use super::BicycleAvailability;
use crate::domain::bike_rental::rental_fleet::{
    BicycleId, RentalFleet, RentalFleetAggregate,
    assess_rental_eligibility::RentalEligibilityOutcome,
};

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
