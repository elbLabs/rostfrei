use rostfrei::ValueObject;

use crate::domain::rental_fleet::{BicycleId, FleetId};

#[derive(ValueObject, Clone, Debug, Eq, PartialEq)]
#[domain(id = "repair-details", label = "Repair details")]
pub struct RepairDetails {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub damage_report: String,
}
