use rostfrei::Apply;

use super::BicycleReservationConfirmed;
use crate::domain::rental_fleet::{RentalFleet, Reservation};

impl Apply<BicycleReservationConfirmed> for RentalFleet {
    fn apply(&mut self, event: &BicycleReservationConfirmed) {
        self.reservations.push(Reservation {
            reservation_id: event.reservation_id.clone(),
            customer_id: event.customer_id.clone(),
            category: event.category.clone(),
            period: event.period.clone(),
            quote: event.quote.clone(),
            collection: None,
        });
    }
}
