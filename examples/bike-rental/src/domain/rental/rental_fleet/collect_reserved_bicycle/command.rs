use crate::domain::{
    Rental,
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "collect-reserved-bicycle", label = "Collect reserved bicycle", context = Rental)]
pub struct CollectReservedBicycle {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub payment_id: PaymentId,
    pub payment_method: String,
}
