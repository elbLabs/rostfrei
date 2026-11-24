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
    assert_eq!(
        BicycleStatus::Available.evaluate(&BicycleRentalTransition::Retire),
        Ok(StateChange::new(
            BicycleStatus::Available,
            BicycleStatus::Retired,
        ))
    );
    assert_eq!(
        BicycleStatus::Rented.evaluate(&BicycleRentalTransition::Retire),
        Ok(StateChange::new(
            BicycleStatus::Rented,
            BicycleStatus::Retired,
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
    let [edge] = descriptor.edges else {
        panic!("rent should have one edge");
    };
    assert_eq!(edge.from, BicycleStatus::Available);
    assert_eq!(edge.to, BicycleStatus::Rented);
}

#[test]
fn exposes_multiple_edges_for_one_logical_transition() {
    let descriptor = BicycleRentalTransition::Retire.descriptor();

    assert_eq!(descriptor.id.local, "retire");
    assert_eq!(descriptor.label, "Retire");
    let [available, rented] = descriptor.edges else {
        panic!("retire should have two edges");
    };
    assert_eq!(available.from, BicycleStatus::Available);
    assert_eq!(available.to, BicycleStatus::Retired);
    assert_eq!(rented.from, BicycleStatus::Rented);
    assert_eq!(rented.to, BicycleStatus::Retired);
}
