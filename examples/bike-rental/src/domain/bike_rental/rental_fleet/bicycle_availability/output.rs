use rostfrei::ValueObject;

use crate::domain::rental_fleet::RentalFleetAggregate;

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-availability",
    label = "Bicycle availability",
    owner = RentalFleetAggregate
)]
pub enum BicycleAvailability {
    Available,
    Unavailable,
}
