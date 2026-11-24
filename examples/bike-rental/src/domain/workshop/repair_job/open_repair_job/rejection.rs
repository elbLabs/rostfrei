use rostfrei::DomainError;

use crate::domain::repair_job::RepairJobId;

#[derive(DomainError, Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[domain(
    id = "repair-job-already-opened",
    label = "Repair job already opened",
    code = "REPAIR_JOB_ALREADY_OPENED",
    message = "This repair job already exists with different details."
)]
pub struct RepairJobAlreadyOpened {
    pub repair_job_id: RepairJobId,
}
