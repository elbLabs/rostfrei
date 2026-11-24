use crate::domain::{
    Rental,
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::{PaymentId, PaymentOutcome},
};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "resolve-bicycle-collection", label = "Resolve bicycle collection", context = Rental)]
pub struct ResolveBicycleCollection {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub payment_id: PaymentId,
    pub outcome: PaymentOutcome,
}
