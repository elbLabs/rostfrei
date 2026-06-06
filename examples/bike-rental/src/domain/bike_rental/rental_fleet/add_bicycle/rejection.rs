use rostfrei::DomainError;

use crate::domain::rental_fleet::BicycleId;

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-already-in-fleet",
    label = "Bicycle already in fleet",
    code = "BICYCLE_ALREADY_IN_FLEET",
    message = "The requested bicycle is already part of this fleet."
)]
pub struct BicycleAlreadyInFleet {
    pub bicycle_id: BicycleId,
}
