use rostfrei::domain_action;

use super::RepairJobAlreadyOpened;
use crate::domain::repair_job::RepairDetails;

#[domain_action(id = "open-repair-job", label = "Open repair job")]
pub trait OpenRepairJobAction {
    fn open_repair_job(&mut self, details: RepairDetails) -> Result<(), RepairJobAlreadyOpened>;
}
