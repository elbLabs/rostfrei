use rostfrei::domain_decisions;

use super::RentalEligibilityOutcome;
use crate::domain::rental_fleet::{BicycleCondition, BicycleStatus, RentalFleetAggregate};

pub struct RentalEligibilityDecisions;

#[domain_decisions(aggregate, group = RentalEligibilityDecisions)]
impl RentalFleetAggregate {
    #[decision(id = "assess-rental-eligibility", label = "Assess rental eligibility")]
    pub(in crate::domain) fn assess_rental_eligibility(
        status: BicycleStatus,
        condition: BicycleCondition,
    ) -> RentalEligibilityOutcome {
        if status == BicycleStatus::Rented {
            return RentalEligibilityOutcome::AlreadyRented;
        }
        if condition == BicycleCondition::MaintenanceRequired {
            return RentalEligibilityOutcome::MaintenanceRequired;
        }
        RentalEligibilityOutcome::Eligible
    }
}
