use rostfrei::Command;

use crate::domain::{
    Rental,
    rental_fleet::{BicycleCategory, FleetId, RentalPeriod, ReservationId},
};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "reserve-bicycle-category", label = "Reserve bicycle category", context = Rental)]
pub struct ReserveBicycleCategory {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub customer_id: String,
    pub category: BicycleCategory,
    pub period: RentalPeriod,
}
