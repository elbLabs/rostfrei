use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::{PaymentId, PaymentOutcome},
};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-collection-resolved",
    label = "Bicycle collection resolved"
)]
pub struct BicycleCollectionResolved {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub payment_id: PaymentId,
    pub outcome: PaymentOutcome,
}
