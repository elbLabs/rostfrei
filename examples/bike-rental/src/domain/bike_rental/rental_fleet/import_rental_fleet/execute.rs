use rostfrei::AggregateInstance;

use super::{
    ImportRentalFleetAction, ImportRentalFleetInput, InvalidRentalFleet, RentalFleetImported,
    imported_fleet, validate,
};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl ImportRentalFleetAction for AggregateInstance<RentalFleetAggregate> {
    fn import_rental_fleet(
        &mut self,
        input: ImportRentalFleetInput,
    ) -> Result<(), InvalidRentalFleet> {
        let candidate = imported_fleet(self.state().fleet_id.clone(), &input.bicycles);
        validate(&candidate)?;

        self.raise(RentalFleetImported {
            fleet_id: self.state().fleet_id.clone(),
            bicycles: input.bicycles,
        });

        Ok(())
    }
}
