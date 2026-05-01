use rostfrei::AggregateInstance;

use super::{ImportRentalFleetAction, ImportRentalFleetInput, RentalFleetImported};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl ImportRentalFleetAction for AggregateInstance<RentalFleetAggregate> {
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput) {
        self.raise(RentalFleetImported {
            fleet_id: self.state().fleet_id.clone(),
            bicycles: input.bicycles,
        });
    }
}
