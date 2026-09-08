use rostfrei::Apply;

use super::BicycleTransferredIn;
use crate::domain::rental_fleet::{Bicycle, BicycleStatus, RentalFleet};

impl Apply<BicycleTransferredIn> for RentalFleet {
    fn apply(&mut self, event: &BicycleTransferredIn) {
        self.bicycles.push(Bicycle::new(
            event.bicycle_id.clone(),
            BicycleStatus::Available,
            event.condition,
        ));
    }
}
