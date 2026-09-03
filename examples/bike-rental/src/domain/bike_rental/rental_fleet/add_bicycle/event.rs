use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleCondition, BicycleId, FleetId};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-added", label = "Bicycle added")]
pub struct BicycleAdded {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub condition: BicycleCondition,
}
