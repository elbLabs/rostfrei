use rostfrei::{
    CommittedDomainEvent, IntegrationCommandMapper, IntegrationEvent, IntegrationEventMapper,
};
use serde::{Deserialize, Serialize};

use crate::{
    rental_fleet::{BicycleDamageReported, BicycleId, FleetId},
    repair_job::{OpenRepairJob, RepairJobId},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BicycleMaintenanceRequested {
    pub source_event_id: String,
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub damage_report: String,
}

impl IntegrationEvent for BicycleMaintenanceRequested {
    const EVENT_NAME: &'static str = "bicycle-maintenance-requested";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some(crate::RENTAL_CONTEXT);
}

pub struct BicycleDamageIntegrationMapper;

impl IntegrationEventMapper<BicycleDamageReported> for BicycleDamageIntegrationMapper {
    type Output = BicycleMaintenanceRequested;

    fn map(&self, event: &CommittedDomainEvent<'_, BicycleDamageReported>) -> Self::Output {
        BicycleMaintenanceRequested {
            source_event_id: event.recorded().event_id().as_str().to_owned(),
            fleet_id: event.event().fleet_id.clone(),
            bicycle_id: event.event().bicycle_id.clone(),
            damage_report: event.event().damage_report.clone(),
        }
    }
}

pub struct OpenRepairJobOnMaintenanceRequest;

impl IntegrationCommandMapper<BicycleMaintenanceRequested> for OpenRepairJobOnMaintenanceRequest {
    type Command = OpenRepairJob;
    type Error = &'static str;

    fn map(&self, event: &BicycleMaintenanceRequested) -> Result<Self::Command, Self::Error> {
        Ok(OpenRepairJob {
            repair_job_id: RepairJobId::new(event.source_event_id.clone())
                .ok_or("maintenance request has no source event identity")?,
            fleet_id: event.fleet_id.clone(),
            bicycle_id: event.bicycle_id.clone(),
            damage_report: event.damage_report.clone(),
        })
    }
}
