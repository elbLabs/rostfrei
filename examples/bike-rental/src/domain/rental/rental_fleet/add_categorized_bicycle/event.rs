use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleCategory, BicycleId, FleetId};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-categorized", label = "Bicycle categorized")]
pub struct BicycleCategorized {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub category: BicycleCategory,
}
