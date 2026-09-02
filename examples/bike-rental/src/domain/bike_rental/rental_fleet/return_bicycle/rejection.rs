use rostfrei::DomainError;

use crate::domain::rental_fleet::BicycleId;

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-not-rented",
    label = "Bicycle not rented",
    code = "BICYCLE_NOT_RENTED",
    message = "The requested bicycle is not currently rented."
)]
pub struct BicycleNotRented {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}
