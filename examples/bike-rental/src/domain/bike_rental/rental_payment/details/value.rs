use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_tariff::RentalQuote,
};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "collection-payment", label = "Collection payment")]
pub struct CollectionPayment {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub customer_id: String,
    pub quote: RentalQuote,
    pub payment_method: String,
}
