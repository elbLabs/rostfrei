use rostfrei::DomainError;

use crate::domain::rental_fleet::{BicycleId, RentalFleetAggregate};

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-not-rented",
    label = "Bicycle not rented",
    owner = RentalFleetAggregate,
    code = "BICYCLE_NOT_RENTED",
    message = "The requested bicycle is not currently rented.",
    json
)]
pub struct BicycleNotRented {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}
