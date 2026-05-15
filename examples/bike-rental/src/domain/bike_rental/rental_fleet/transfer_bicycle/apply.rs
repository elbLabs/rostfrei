use rostfrei::Apply;

use super::{BicycleTransferredIn, BicycleTransferredOut};
use crate::domain::rental_fleet::{Bicycle, BicycleStatus, RentalFleet};

impl Apply<BicycleTransferredOut> for RentalFleet {
    fn apply(&mut self, event: &BicycleTransferredOut) {
        self.bicycles
            .retain(|bicycle| bicycle.bicycle_id() != &event.bicycle_id);
    }
}

impl Apply<BicycleTransferredIn> for RentalFleet {
    fn apply(&mut self, event: &BicycleTransferredIn) {
        self.bicycles.push(Bicycle::new(
            event.bicycle_id.clone(),
            BicycleStatus::Available,
            event.condition,
        ));
    }
}
