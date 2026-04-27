use rostfrei::domain_actions;

use super::{ImportRentalFleetInput, RentalFleetImported};

#[domain_actions(aggregate(instance = ImportRentalFleetActions))]
pub trait ImportRentalFleetActionContract {
    #[action(
        id = "import-rental-fleet",
        label = "Import rental fleet",
        raises = [RentalFleetImported]
    )]
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput);
}
