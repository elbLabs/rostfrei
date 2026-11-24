use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleCategory, BicycleCondition, BicycleId, FleetId};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-transferred-out", label = "Bicycle transferred out")]
pub struct BicycleTransferredOut {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub to_fleet_id: FleetId,
    pub condition: BicycleCondition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<BicycleCategory>,
}
