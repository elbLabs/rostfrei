use rostfrei::Command;

use crate::domain::{
    BikeRental,
    rental_fleet::{BicycleId, FleetId},
    repair_job::RepairJobId,
};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "open-repair-job", label = "Open repair job", context = BikeRental)]
pub struct OpenRepairJob {
    pub repair_job_id: RepairJobId,
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub damage_report: String,
}
