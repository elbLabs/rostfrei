use rostfrei::{DomainIdentity, Entity, ValueObject, domain_actions};
use serde::{Deserialize, Serialize};

use super::RentalFleetAggregate;

#[allow(
    clippy::struct_field_names,
    reason = "bicycle_id is the canonical domain identity name"
)]
#[derive(Entity, Debug)]
#[domain(
    id = "bicycle",
    label = "Bicycle",
    owner = RentalFleetAggregate,
    actions = [BicycleStatusActions]
)]
pub struct Bicycle {
    #[domain(identity)]
    bicycle_id: BicycleId,
    #[domain(value_object)]
    status: BicycleStatus,
    #[domain(value_object)]
    condition: BicycleCondition,
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

    pub(super) const fn mark_available(&mut self) {
        self.status = BicycleStatus::Available;
    }
}

#[derive(
    DomainIdentity, Clone, Debug, Deserialize, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize,
)]
#[domain(owner = Bicycle)]
#[serde(try_from = "String")]
pub struct BicycleId(String);

impl BicycleId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.trim().is_empty() && value.trim() == value).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BicycleId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("bicycle ID must be non-empty and trimmed")
    }
}

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-status",
    label = "Bicycle status",
    owner = RentalFleetAggregate
)]
#[serde(rename_all = "kebab-case")]
pub enum BicycleStatus {
    Available,
    Rented,
}

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-condition",
    label = "Bicycle condition",
    owner = RentalFleetAggregate
)]
#[serde(rename_all = "kebab-case")]
pub enum BicycleCondition {
    Serviceable,
    MaintenanceRequired,
}

#[domain_actions(entity)]
pub(super) trait BicycleStatusActions {
    #[action(id = "mark-rented", label = "Mark rented")]
    fn mark_rented(&mut self, input: BicycleStatus);
}

impl BicycleStatusActions for Bicycle {
    fn mark_rented(&mut self, input: BicycleStatus) {
        self.status = input;
    }
}
