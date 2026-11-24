use rostfrei::{Initialize, StreamId};

use super::{RepairJob, RepairJobAggregate, RepairJobId};

impl Initialize<RepairJobAggregate> for RepairJob {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            repair_job_id: RepairJobId::from(stream_id.aggregate_id()),
            details: None,
        }
    }
}
