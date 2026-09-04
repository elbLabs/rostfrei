use rostfrei::domain_decision;

use super::RentalEligibilityOutcome;

#[domain_decision(id = "assess-rental-eligibility", label = "Assess rental eligibility")]
pub(in crate::domain) trait RentalEligibilityDecision {
    fn assess_rental_eligibility(&self) -> RentalEligibilityOutcome;
}
