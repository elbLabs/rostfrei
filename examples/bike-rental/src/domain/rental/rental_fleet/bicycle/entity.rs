use rostfrei::{Entity, EntityDefinition};

use super::{BicycleCategory, BicycleCondition, BicycleId, BicycleStatus};
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
    pub(in crate::domain::rental::rental_fleet) category: Option<BicycleCategory>,
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
            category: None,
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

    pub const fn category(&self) -> Option<&BicycleCategory> {
        self.category.as_ref()
    }

    pub(in crate::domain::rental::rental_fleet) const fn apply_rented(&mut self) {
        self.status = BicycleStatus::Rented;
    }

    pub(in crate::domain::rental::rental_fleet) const fn apply_collection_requested(&mut self) {
        self.status = BicycleStatus::AwaitingPayment;
    }

    pub(in crate::domain::rental::rental_fleet) const fn apply_returned(&mut self) {
        self.status = BicycleStatus::Available;
    }

    pub(in crate::domain::rental::rental_fleet) const fn apply_damage_reported(&mut self) {
        self.condition = BicycleCondition::MaintenanceRequired;
    }

    pub(in crate::domain::rental::rental_fleet) const fn apply_retired(&mut self) {
        self.status = BicycleStatus::Retired;
    }
}
