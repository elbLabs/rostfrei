use crate::domain::{
    Finance,
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "assess-rental-return", label = "Assess rental return", context = Finance)]
pub struct AssessRentalReturn {
    pub payment_id: PaymentId,
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub eligible: bool,
}
