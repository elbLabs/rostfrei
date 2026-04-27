use rostfrei::{Entity, EntityDefinition};

use super::{BicycleCondition, BicycleId, BicycleStatus};
use crate::domain::rental_fleet::RentalFleetAggregate;

#[allow(
    clippy::struct_field_names,
    reason = "bicycle_id is the canonical domain identity name"
)]
#[derive(Entity, Debug)]
#[domain(id = "bicycle", label = "Bicycle")]
pub struct Bicycle {
    #[domain(identity)]
    pub(super) bicycle_id: BicycleId,
    #[domain(value_object)]
    pub(super) status: BicycleStatus,
    #[domain(value_object)]
    pub(super) condition: BicycleCondition,
}

impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;
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
