use rostfrei::Entity;

use super::{BicycleCondition, BicycleId, BicycleRentalLifecycle, BicycleStatus};
use crate::domain::rental_fleet::RentalFleetAggregate;

#[allow(
    clippy::struct_field_names,
    reason = "bicycle_id is the canonical domain identity name"
)]
#[derive(Entity, Debug)]
#[domain(
    id = "bicycle",
    label = "Bicycle",
    owner = RentalFleetAggregate,
    actions = [
        super::mark_rented::MarkRentedAction,
        super::mark_available::MarkAvailableAction
    ],
    lifecycle = BicycleRentalLifecycle
)]
pub struct Bicycle {
    #[domain(identity)]
    pub(super) bicycle_id: BicycleId,
    #[domain(value_object)]
    pub(super) status: BicycleStatus,
    #[domain(value_object)]
    pub(super) condition: BicycleCondition,
}

impl Bicycle {
    pub const fn new(
        bicycle_id: BicycleId,
        status: BicycleStatus,
        condition: BicycleCondition,
    ) -> Self {
        Self {
            bicycle_id,
            status,
            condition,
        }
    }

    pub const fn bicycle_id(&self) -> &BicycleId {
        &self.bicycle_id
    }

    pub const fn status(&self) -> BicycleStatus {
        self.status
    }

    pub const fn condition(&self) -> BicycleCondition {
        self.condition
    }
}
