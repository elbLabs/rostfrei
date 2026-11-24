use rostfrei::domain_action;

use super::ReservationRejected;
use crate::domain::rental_fleet::{BicycleCategory, RentalPeriod, ReservationId};
use crate::domain::rental_tariff::RentalQuote;

#[domain_action(id = "reserve-bicycle-category", label = "Reserve bicycle category")]
pub trait ReserveBicycleCategoryAction {
    fn reserve_bicycle_category(
        &mut self,
        reservation_id: ReservationId,
        customer_id: String,
        category: BicycleCategory,
        period: RentalPeriod,
        quote: Option<RentalQuote>,
    ) -> Result<(), ReservationRejected>;
}
