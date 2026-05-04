use rostfrei::Apply;

use super::BicycleRetired;
use crate::domain::rental_fleet::RentalFleet;

impl Apply<BicycleRetired> for RentalFleet {
    fn apply(&mut self, event: &BicycleRetired) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.apply_retired();
        }
    }
}
