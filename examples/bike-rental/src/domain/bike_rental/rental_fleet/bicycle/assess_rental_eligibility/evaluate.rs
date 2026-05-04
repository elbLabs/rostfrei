use rostfrei::LifecycleState;

use super::{RentalEligibilityDecision, RentalEligibilityOutcome};
use crate::domain::rental_fleet::{Bicycle, BicycleCondition, BicycleRentalTransition};

impl RentalEligibilityDecision for Bicycle {
    fn assess_rental_eligibility(&self) -> RentalEligibilityOutcome {
        if self
            .status()
            .evaluate(&BicycleRentalTransition::Rent)
            .is_err()
        {
            return RentalEligibilityOutcome::AlreadyRented;
        }
        if self.condition() == BicycleCondition::MaintenanceRequired {
            return RentalEligibilityOutcome::MaintenanceRequired;
        }
        RentalEligibilityOutcome::Eligible
    }
}
