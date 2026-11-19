use rostfrei::Apply;

use super::BicycleDamageReported;
use crate::domain::rental_fleet::RentalFleet;

impl Apply<BicycleDamageReported> for RentalFleet {
    fn apply(&mut self, event: &BicycleDamageReported) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.apply_damage_reported();
        }
    }
}
