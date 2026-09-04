use rostfrei::DomainError;

use crate::domain::rental_fleet::BicycleId;

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-cannot-be-retired",
    label = "Bicycle cannot be retired",
    code = "BICYCLE_CANNOT_BE_RETIRED",
    message = "The requested bicycle cannot be retired from its current state."
)]
pub struct BicycleCannotBeRetired {
    pub bicycle_id: BicycleId,
}
