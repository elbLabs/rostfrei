use rostfrei::AggregateInstance;

use super::{ImportRentalFleetActions, ImportRentalFleetInput, RentalFleetImported};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl ImportRentalFleetActions for AggregateInstance<RentalFleetAggregate> {
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput) {
        self.raise(RentalFleetImported {
            fleet_id: self.state().fleet_id.clone(),
            bicycles: input.bicycles,
        });
    }
}
