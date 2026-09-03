use rostfrei::domain_action;

use super::BicycleUnavailable;
use crate::domain::rental_fleet::BicycleId;

#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
pub trait RentBicycleAction {
    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable>;
}
