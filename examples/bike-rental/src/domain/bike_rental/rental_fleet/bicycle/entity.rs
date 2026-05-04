use rostfrei::{Entity, EntityDefinition, StateTransition as _};

use super::{BicycleCondition, BicycleId, BicycleRentalTransition, BicycleStatus};
use crate::domain::rental_fleet::RentalFleetAggregate;

#[allow(
    clippy::struct_field_names,
    reason = "bicycle_id is the canonical domain identity name"
)]
#[derive(Entity, Debug)]
#[domain(id = "bicycle", label = "Bicycle")]
pub struct Bicycle {
    pub(super) bicycle_id: BicycleId,
    pub(super) status: BicycleStatus,
    pub(super) condition: BicycleCondition,
}

impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;

    fn identity(&self) -> &Self::Identity {
        &self.bicycle_id
    }
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

    pub(in crate::domain::bike_rental::rental_fleet) fn apply_transition(
        &mut self,
        transition: BicycleRentalTransition,
    ) {
        self.status = transition.descriptor().to;
    }
}
