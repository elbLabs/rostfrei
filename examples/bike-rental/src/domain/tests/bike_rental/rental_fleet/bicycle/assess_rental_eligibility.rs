use crate::domain::rental_fleet::{
    Bicycle, BicycleCondition, BicycleId, BicycleStatus,
    assess_rental_eligibility::{RentalEligibilityDecision, RentalEligibilityOutcome},
};

#[rostfrei::domain_decision_test(<Bicycle as RentalEligibilityDecision>::DESCRIPTOR)]
fn returns_first_class_outcomes() {
    assert_eq!(
        bicycle(BicycleStatus::Available, BicycleCondition::Serviceable)
            .assess_rental_eligibility(),
        RentalEligibilityOutcome::Eligible
    );
    assert_eq!(
        bicycle(BicycleStatus::Rented, BicycleCondition::Serviceable).assess_rental_eligibility(),
        RentalEligibilityOutcome::AlreadyRented
    );
    assert_eq!(
        bicycle(
            BicycleStatus::Available,
            BicycleCondition::MaintenanceRequired,
        )
        .assess_rental_eligibility(),
        RentalEligibilityOutcome::MaintenanceRequired
    );
}

fn bicycle(status: BicycleStatus, condition: BicycleCondition) -> Bicycle {
    Bicycle::new(
        BicycleId::new("bike-1").expect("test bicycle ID should be valid"),
        status,
        condition,
    )
}
