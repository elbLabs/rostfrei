use super::{RentalEligibilityDecision, RentalEligibilityOutcome};
use crate::domain::rental_fleet::{BicycleCondition, BicycleStatus, RentalFleetAggregate};

impl RentalEligibilityDecision for RentalFleetAggregate {
    fn assess_rental_eligibility(
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
