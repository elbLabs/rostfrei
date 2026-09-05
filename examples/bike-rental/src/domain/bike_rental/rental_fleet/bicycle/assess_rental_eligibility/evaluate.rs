use rostfrei::LifecycleState;

use super::{RentalEligibilityOutcome, RentalEligibilityPolicy};
use crate::domain::rental_fleet::{Bicycle, BicycleCondition, BicycleRentalTransition};

impl RentalEligibilityPolicy for Bicycle {
    fn assess_rental_eligibility(&self) -> RentalEligibilityOutcome {
        if self
            .status()
            .evaluate(&BicycleRentalTransition::Rent)
            .is_err()
        {
            return RentalEligibilityOutcome::UnavailableStatus;
        }
        if self.condition() == BicycleCondition::MaintenanceRequired {
            return RentalEligibilityOutcome::MaintenanceRequired;
        }
        RentalEligibilityOutcome::Eligible
    }
}
