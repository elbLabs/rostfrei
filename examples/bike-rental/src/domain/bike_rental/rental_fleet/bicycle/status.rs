use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::RentalFleetAggregate;

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-status",
    label = "Bicycle status",
    owner = RentalFleetAggregate
)]
#[serde(rename_all = "kebab-case")]
pub enum BicycleStatus {
    Available,
    Rented,
}
