use rostfrei::ValueObject;

use super::ImportedBicycle;
use crate::domain::rental_fleet::RentalFleetAggregate;

#[derive(ValueObject, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "import-rental-fleet-input",
    label = "Import rental fleet input",
    owner = RentalFleetAggregate
)]
pub struct ImportRentalFleetInput {
    #[domain(value_object)]
    pub(super) bicycles: Vec<ImportedBicycle>,
}

impl ImportRentalFleetInput {
    pub const fn new(bicycles: Vec<ImportedBicycle>) -> Self {
        Self { bicycles }
    }
}
