use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "rental-payment-identity-conflict-recorded",
    label = "Rental payment identity conflict recorded"
)]
pub struct RentalPaymentIdentityConflictRecorded {
    pub payment_id: PaymentId,
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
}
