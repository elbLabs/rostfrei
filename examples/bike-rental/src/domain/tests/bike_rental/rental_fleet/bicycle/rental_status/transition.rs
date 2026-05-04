use rostfrei::{
    EntityLifecycleId, EntityLifecycleStateId, EntityLifecycleTransitionId, InvalidStateTransition,
    LifecycleState, StateChange, StateTransition,
};

use crate::domain::rental_fleet::{BicycleRentalTransition, BicycleStatus};

const LIFECYCLE: EntityLifecycleId = EntityLifecycleId("rental-status");

#[test]
fn evaluates_legal_rental_transitions() {
    assert_eq!(
        BicycleStatus::Available.evaluate(&BicycleRentalTransition::Rent),
        Ok(StateChange::new(
            BicycleStatus::Available,
            BicycleStatus::Rented,
        ))
    );
    assert_eq!(
        BicycleStatus::Rented.evaluate(&BicycleRentalTransition::Return),
        Ok(StateChange::new(
            BicycleStatus::Rented,
            BicycleStatus::Available,
        ))
    );
}

#[test]
fn rejects_transitions_from_the_wrong_state() {
    assert_eq!(
        BicycleStatus::Rented.evaluate(&BicycleRentalTransition::Rent),
        Err(InvalidStateTransition::new(
            EntityLifecycleStateId {
                lifecycle: LIFECYCLE,
                local: "rented",
            },
            EntityLifecycleTransitionId {
                lifecycle: LIFECYCLE,
                local: "rent",
            },
        ))
    );
}

#[test]
fn exposes_stable_transition_metadata() {
    let descriptor = BicycleRentalTransition::Rent.descriptor();

    assert_eq!(descriptor.id.local, "rent");
    assert_eq!(descriptor.label, "Rent");
    assert_eq!(descriptor.from, BicycleStatus::Available);
    assert_eq!(descriptor.to, BicycleStatus::Rented);
}
