use rostfrei::domain_actions;

use super::ImportRentalFleetInput;

#[domain_actions(aggregate(instance = ImportRentalFleetActions))]
pub trait ImportRentalFleetActionContract {
    #[action(id = "import-rental-fleet", label = "Import rental fleet")]
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput);
}
