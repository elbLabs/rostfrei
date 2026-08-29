use rostfrei::{DecisionOutcome, domain_decisions};

use super::{BicycleCondition, BicycleStatus, RentalFleetAggregate};

#[derive(DecisionOutcome, Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RentalEligibilityOutcome {
    #[outcome(id = "eligible", label = "Eligible")]
    Eligible,
    #[outcome(id = "already-rented", label = "Already rented")]
    AlreadyRented,
    #[outcome(id = "maintenance-required", label = "Maintenance required")]
    MaintenanceRequired,
}

pub(super) struct RentalEligibilityDecisions;

#[domain_decisions(aggregate, group = RentalEligibilityDecisions)]
impl RentalFleetAggregate {
    #[decision(id = "assess-rental-eligibility", label = "Assess rental eligibility")]
    pub(super) fn assess_rental_eligibility(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rental_eligibility_returns_first_class_outcomes() {
        assert_eq!(
            RentalFleetAggregate::assess_rental_eligibility(
                BicycleStatus::Available,
                BicycleCondition::Serviceable,
            ),
            RentalEligibilityOutcome::Eligible
        );
        assert_eq!(
            RentalFleetAggregate::assess_rental_eligibility(
                BicycleStatus::Rented,
                BicycleCondition::Serviceable,
            ),
            RentalEligibilityOutcome::AlreadyRented
        );
        assert_eq!(
            RentalFleetAggregate::assess_rental_eligibility(
                BicycleStatus::Available,
                BicycleCondition::MaintenanceRequired,
            ),
            RentalEligibilityOutcome::MaintenanceRequired
        );
    }
}
