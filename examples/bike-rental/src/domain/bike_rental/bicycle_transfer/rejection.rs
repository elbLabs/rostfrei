use rostfrei::DomainError;

use super::BicycleTransferRejectionReason;
use crate::domain::rental_fleet::{BicycleId, FleetId};

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-transfer-rejected",
    label = "Bicycle transfer rejected",
    code = "BICYCLE_TRANSFER_REJECTED",
    message = "The requested bicycle transfer is not allowed."
)]
pub struct BicycleTransferRejected {
    pub bicycle_id: BicycleId,
    pub from_fleet_id: FleetId,
    pub to_fleet_id: FleetId,
    pub reason: BicycleTransferRejectionReason,
}
