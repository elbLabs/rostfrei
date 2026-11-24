use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-collection-cancelled",
    label = "Bicycle collection cancelled"
)]
pub struct BicycleCollectionCancelled {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub payment_id: PaymentId,
}
