use rostfrei::DomainError;

use crate::domain::rental_fleet::BicycleId;

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-unavailable",
    label = "Bicycle unavailable",
    code = "BICYCLE_UNAVAILABLE",
    message = "The requested bicycle cannot currently be rented."
)]
pub struct BicycleUnavailable {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}
