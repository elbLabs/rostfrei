use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::{CollectionPayment, PaymentId},
    rental_tariff::RentalQuote,
};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-collection-requested",
    label = "Bicycle collection requested"
)]
pub struct BicycleCollectionRequested {
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub payment_id: PaymentId,
    pub customer_id: String,
    pub quote: RentalQuote,
    pub payment_method: String,
}

impl BicycleCollectionRequested {
    pub fn payment_details(&self) -> CollectionPayment {
        CollectionPayment {
            fleet_id: self.fleet_id.clone(),
            reservation_id: self.reservation_id.clone(),
            bicycle_id: self.bicycle_id.clone(),
            customer_id: self.customer_id.clone(),
            quote: self.quote.clone(),
            payment_method: self.payment_method.clone(),
        }
    }
}
