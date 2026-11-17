use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleId, FleetId};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-damage-reported", label = "Bicycle damage reported")]
pub struct BicycleDamageReported {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub damage_report: String,
}
