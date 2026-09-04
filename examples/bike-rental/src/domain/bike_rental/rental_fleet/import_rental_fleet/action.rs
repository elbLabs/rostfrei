use rostfrei::domain_action;

use super::{ImportRentalFleetInput, InvalidRentalFleet};

#[domain_action(id = "import-rental-fleet", label = "Import rental fleet")]
pub trait ImportRentalFleetAction {
    fn import_rental_fleet(
        &mut self,
        input: ImportRentalFleetInput,
    ) -> Result<(), InvalidRentalFleet>;
}
