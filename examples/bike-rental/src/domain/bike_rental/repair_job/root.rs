use rostfrei::{Entity, EntityDefinition};

use super::{RepairDetails, RepairJobAggregate, RepairJobId};

#[derive(Entity, Debug)]
#[domain(id = "repair-job-root", label = "Workshop repair job")]
pub struct RepairJob {
    pub(super) repair_job_id: RepairJobId,
    pub(super) details: Option<RepairDetails>,
}

impl EntityDefinition for RepairJob {
    type Owner = RepairJobAggregate;
    type Identity = RepairJobId;

    fn identity(&self) -> &Self::Identity {
        &self.repair_job_id
    }
}

impl RepairJob {
    pub const fn repair_job_id(&self) -> &RepairJobId {
        &self.repair_job_id
    }

    pub const fn details(&self) -> Option<&RepairDetails> {
        self.details.as_ref()
    }
}
