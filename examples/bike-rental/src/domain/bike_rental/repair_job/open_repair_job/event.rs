use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::{
    rental_fleet::{BicycleId, FleetId},
    repair_job::RepairJobId,
};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "repair-job-opened", label = "Repair job opened")]
pub struct RepairJobOpened {
    pub repair_job_id: RepairJobId,
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub damage_report: String,
}
