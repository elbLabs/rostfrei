use rostfrei::Apply;

use super::{RentalFleetImported, imported_fleet};
use crate::domain::rental_fleet::RentalFleet;

impl Apply<RentalFleetImported> for RentalFleet {
    fn apply(&mut self, event: &RentalFleetImported) {
        *self = imported_fleet(self.fleet_id().clone(), &event.bicycles);
    }
}
