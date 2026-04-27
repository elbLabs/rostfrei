use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleCondition, BicycleId, FleetId};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-added", label = "Bicycle added")]
pub struct BicycleAdded {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(identity)]
    pub bicycle_id: BicycleId,
    #[domain(value_object)]
    pub condition: BicycleCondition,
}
