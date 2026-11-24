use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::PaymentId,
    rental_tariff::RentalQuote,
};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-payment-approved", label = "Rental payment approved")]
pub struct RentalPaymentApproved {
    pub payment_id: PaymentId,
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub provider_reference: String,
    pub quote: RentalQuote,
}
