use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{OpenRepairJob, OpenRepairJobAction as _, RepairJobAlreadyOpened};
use crate::domain::repair_job::{RepairDetails, RepairJobAggregate};

pub struct OpenRepairJobHandler;

#[async_trait]
impl CommandHandler<OpenRepairJob> for OpenRepairJobHandler {
    type Rejection = RepairJobAlreadyOpened;

    async fn handle(
        &self,
        command: &OpenRepairJob,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut job = execution
            .load::<RepairJobAggregate>(command.repair_job_id.as_str())
            .await?;
        match job.open_repair_job(RepairDetails {
            fleet_id: command.fleet_id.clone(),
            bicycle_id: command.bicycle_id.clone(),
            damage_report: command.damage_report.clone(),
        }) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
