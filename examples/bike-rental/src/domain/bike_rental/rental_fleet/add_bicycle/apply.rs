use rostfrei::Apply;

use super::BicycleAdded;
use crate::domain::rental_fleet::{Bicycle, BicycleStatus, RentalFleet};

impl Apply<BicycleAdded> for RentalFleet {
    fn apply(&mut self, event: &BicycleAdded) {
        self.bicycles.push(Bicycle::new(
            event.bicycle_id.clone(),
            BicycleStatus::Available,
            event.condition,
        ));
    }
}
