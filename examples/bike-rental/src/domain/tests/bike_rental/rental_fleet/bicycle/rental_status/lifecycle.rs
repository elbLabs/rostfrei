use rostfrei::{
    EntityLifecycleDescriptor, EntityLifecycleId, EntityLifecycleStateDescriptor,
    EntityLifecycleStateId, EntityLifecycleType, LifecycleState,
};

use crate::domain::rental_fleet::BicycleStatus;

#[test]
fn declares_rental_status_metadata_and_initial_state() {
    const LIFECYCLE: EntityLifecycleId = EntityLifecycleId("rental-status");

    assert_eq!(
        BicycleStatus::DESCRIPTOR,
        EntityLifecycleDescriptor {
            id: LIFECYCLE,
            label: "Bicycle rental status",
            initial: EntityLifecycleStateId {
                lifecycle: LIFECYCLE,
                local: "available",
            },
            states: &[
                EntityLifecycleStateDescriptor {
                    id: EntityLifecycleStateId {
                        lifecycle: LIFECYCLE,
                        local: "available",
                    },
                    label: "Available",
                },
                EntityLifecycleStateDescriptor {
                    id: EntityLifecycleStateId {
                        lifecycle: LIFECYCLE,
                        local: "rented",
                    },
                    label: "Rented",
                },
            ],
        }
    );
    assert_eq!(BicycleStatus::INITIAL, BicycleStatus::Available);
    assert_eq!(
        BicycleStatus::Rented.state_id(),
        EntityLifecycleStateId {
            lifecycle: LIFECYCLE,
            local: "rented",
        }
    );
}

#[test]
fn preserves_status_wire_values() {
    assert_eq!(
        serde_json::to_string(&BicycleStatus::Available).expect("status should serialize"),
        "\"available\""
    );
    assert_eq!(
        serde_json::from_str::<BicycleStatus>("\"rented\"").expect("status should deserialize"),
        BicycleStatus::Rented
    );
}
