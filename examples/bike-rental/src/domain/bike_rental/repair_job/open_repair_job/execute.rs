use rostfrei::AggregateInstance;

use super::{OpenRepairJobAction, RepairJobAlreadyOpened, RepairJobOpened};
use crate::domain::repair_job::{RepairDetails, RepairJobAggregate};

impl OpenRepairJobAction for AggregateInstance<RepairJobAggregate> {
    fn open_repair_job(&mut self, details: RepairDetails) -> Result<(), RepairJobAlreadyOpened> {
        if let Some(existing) = self.state().details() {
            return if existing == &details {
                Ok(())
            } else {
                Err(RepairJobAlreadyOpened {
                    repair_job_id: self.state().repair_job_id().clone(),
                })
            };
        }
        self.raise(RepairJobOpened {
            repair_job_id: self.state().repair_job_id().clone(),
            fleet_id: details.fleet_id,
            bicycle_id: details.bicycle_id,
            damage_report: details.damage_report,
        });
        Ok(())
    }
}
