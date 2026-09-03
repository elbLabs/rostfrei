use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleId, FleetId};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-returned", label = "Bicycle returned")]
pub struct BicycleReturned {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
}
