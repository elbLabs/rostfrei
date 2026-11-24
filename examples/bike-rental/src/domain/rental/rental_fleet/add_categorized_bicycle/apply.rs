use rostfrei::Apply;

use super::BicycleCategorized;
use crate::domain::rental_fleet::RentalFleet;

impl Apply<BicycleCategorized> for RentalFleet {
    fn apply(&mut self, event: &BicycleCategorized) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bike| bike.bicycle_id() == &event.bicycle_id)
        {
            bicycle.category = Some(event.category.clone());
        }
    }
}
