use rostfrei::domain_decision;

use super::RentalEligibilityOutcome;
use crate::domain::rental_fleet::{BicycleCondition, BicycleStatus};

#[domain_decision(id = "assess-rental-eligibility", label = "Assess rental eligibility")]
pub(in crate::domain) trait RentalEligibilityDecision {
    fn assess_rental_eligibility(
        status: BicycleStatus,
        condition: BicycleCondition,
    ) -> RentalEligibilityOutcome;
}
