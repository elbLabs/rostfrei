use crate::domain::rental_fleet::{
    BicycleCondition, BicycleStatus, RentalFleetAggregate,
    assess_rental_eligibility::{RentalEligibilityDecision, RentalEligibilityOutcome},
};

#[rostfrei::domain_decision_test(
    <RentalFleetAggregate as RentalEligibilityDecision>::DESCRIPTOR
)]
fn returns_first_class_outcomes() {
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
