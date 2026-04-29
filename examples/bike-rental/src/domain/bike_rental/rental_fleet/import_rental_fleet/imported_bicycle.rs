use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleCondition, BicycleId, BicycleStatus};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportedBicycle {
    pub bicycle_id: BicycleId,
    pub status: BicycleStatus,
    pub condition: BicycleCondition,
}
