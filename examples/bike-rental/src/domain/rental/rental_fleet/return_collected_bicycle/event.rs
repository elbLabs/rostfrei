use crate::domain::{
    rental_fleet::{BicycleId, FleetId, RentalReturn, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "reserved-bicycle-returned", label = "Reserved bicycle returned")]
pub struct ReservedBicycleReturned {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub payment_id: PaymentId,
    pub details: RentalReturn,
    pub eligible: bool,
}
