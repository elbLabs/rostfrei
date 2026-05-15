use rostfrei::Apply;

use super::BicycleTransferredOut;
use crate::domain::rental_fleet::RentalFleet;

impl Apply<BicycleTransferredOut> for RentalFleet {
    fn apply(&mut self, event: &BicycleTransferredOut) {
        self.bicycles
            .retain(|bicycle| bicycle.bicycle_id() != &event.bicycle_id);
    }
}
