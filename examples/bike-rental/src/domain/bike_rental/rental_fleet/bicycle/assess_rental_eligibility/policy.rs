use rostfrei::domain_policy;

use super::RentalEligibilityOutcome;

#[domain_policy(id = "assess-rental-eligibility", label = "Assess rental eligibility")]
pub(in crate::domain) trait RentalEligibilityPolicy {
    fn assess_rental_eligibility(&self) -> RentalEligibilityOutcome;
}
