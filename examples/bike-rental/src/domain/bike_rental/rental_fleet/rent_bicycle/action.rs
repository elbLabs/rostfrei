use rostfrei::domain_actions;

use super::{BicycleRented, BicycleUnavailable};
use crate::domain::rental_fleet::BicycleId;

#[domain_actions(aggregate(instance = RentBicycleActions))]
pub trait RentBicycleActionContract {
    #[action(
        id = "rent-bicycle",
        label = "Rent bicycle",
        raises = [BicycleRented]
    )]
    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable>;
}
