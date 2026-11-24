use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{BicycleCategory, FleetId, RentalPeriod, ReservationId};
use crate::domain::rental_tariff::RentalQuote;

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-reservation-confirmed",
    label = "Bicycle reservation confirmed"
)]
pub struct BicycleReservationConfirmed {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub customer_id: String,
    pub category: BicycleCategory,
    pub period: RentalPeriod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<RentalQuote>,
}
