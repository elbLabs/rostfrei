use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-status", label = "Bicycle status")]
#[serde(rename_all = "kebab-case")]
pub enum BicycleStatus {
    Available,
    Rented,
}
