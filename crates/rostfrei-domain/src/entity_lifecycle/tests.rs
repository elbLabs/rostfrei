use super::{
    EntityLifecycleDescriptor, EntityLifecycleId, EntityLifecycleStateDescriptor,
    EntityLifecycleStateId, EntityLifecycleTransitionId, EntityLifecycleType,
    InvalidStateTransition, LifecycleState, StateChange, StateTransition,
    StateTransitionDescriptor,
};

const LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId("rental-status");
const AVAILABLE_ID: EntityLifecycleStateId = state_id("available");
const RENTED_ID: EntityLifecycleStateId = state_id("rented");
const RENT_ID: EntityLifecycleTransitionId = EntityLifecycleTransitionId {
    lifecycle: LIFECYCLE_ID,
    local: "rent",
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RentalStatus {
    Available,
    Rented,
}

impl EntityLifecycleType for RentalStatus {
    const DESCRIPTOR: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
        id: LIFECYCLE_ID,
        label: "Rental status",
        initial: AVAILABLE_ID,
        states: &[
            EntityLifecycleStateDescriptor {
                id: AVAILABLE_ID,
                label: "Available",
            },
            EntityLifecycleStateDescriptor {
                id: RENTED_ID,
                label: "Rented",
            },
        ],
    };
}

impl LifecycleState for RentalStatus {
    const INITIAL: Self = Self::Available;

    fn state_id(self) -> EntityLifecycleStateId {
        match self {
            Self::Available => AVAILABLE_ID,
            Self::Rented => RENTED_ID,
        }
    }
}

enum RentalTransition {
    Rent,
}

static RENT_DESCRIPTOR: StateTransitionDescriptor<RentalStatus> = StateTransitionDescriptor {
    id: RENT_ID,
    label: "Rent",
    from: RentalStatus::Available,
    to: RentalStatus::Rented,
};

impl StateTransition for RentalTransition {
    type State = RentalStatus;

    const DESCRIPTORS: &'static [StateTransitionDescriptor<Self::State>] = &[RENT_DESCRIPTOR];

    fn descriptor(&self) -> &'static StateTransitionDescriptor<Self::State> {
        match self {
            Self::Rent => &RENT_DESCRIPTOR,
        }
    }
}

#[test]
fn evaluates_a_valid_transition() {
    assert_eq!(
        RentalStatus::Available.evaluate(&RentalTransition::Rent),
        Ok(StateChange::new(
            RentalStatus::Available,
            RentalStatus::Rented,
        )),
    );
}

#[test]
fn rejects_a_transition_from_the_wrong_state() {
    let rejection = RentalStatus::Rented.evaluate(&RentalTransition::Rent);

    assert_eq!(
        rejection,
        Err(InvalidStateTransition::new(RENTED_ID, RENT_ID)),
    );
}

#[test]
fn describes_an_invalid_transition() {
    assert_eq!(
        InvalidStateTransition::new(RENTED_ID, RENT_ID).to_string(),
        "transition `rent` is not valid from state `rented` in lifecycle `rental-status`",
    );
}

const fn state_id(local: &'static str) -> EntityLifecycleStateId {
    EntityLifecycleStateId {
        lifecycle: LIFECYCLE_ID,
        local,
    }
}
