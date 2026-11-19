use rostfrei::Apply;

use super::RepairJobOpened;
use crate::domain::repair_job::{RepairDetails, RepairJob};

impl Apply<RepairJobOpened> for RepairJob {
    fn apply(&mut self, event: &RepairJobOpened) {
        self.details = Some(RepairDetails {
            fleet_id: event.fleet_id.clone(),
            bicycle_id: event.bicycle_id.clone(),
            damage_report: event.damage_report.clone(),
        });
    }
}
