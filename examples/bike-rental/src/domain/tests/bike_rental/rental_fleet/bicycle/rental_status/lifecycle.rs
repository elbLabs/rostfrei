use rostfrei::{
    EntityLifecycleDescriptor, EntityLifecycleId, EntityLifecycleStateDescriptor,
    EntityLifecycleStateId, EntityLifecycleType,
};

use crate::domain::rental_fleet::BicycleRentalLifecycle;

#[test]
fn declares_ordered_rental_status_metadata() {
    const LIFECYCLE: EntityLifecycleId = EntityLifecycleId("rental-status");

    assert_eq!(
        BicycleRentalLifecycle::DESCRIPTOR,
        EntityLifecycleDescriptor {
            id: LIFECYCLE,
            label: "Bicycle rental status",
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
}
