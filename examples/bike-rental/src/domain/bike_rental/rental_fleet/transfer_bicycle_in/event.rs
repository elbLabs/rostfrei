use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleCondition, BicycleId, FleetId};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-transferred-in", label = "Bicycle transferred in")]
pub struct BicycleTransferredIn {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub from_fleet_id: FleetId,
    pub condition: BicycleCondition,
}
