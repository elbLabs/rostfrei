use rostfrei::EntityLifecycle;
use serde::{Deserialize, Serialize};

#[derive(EntityLifecycle, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-status", label = "Bicycle rental status")]
#[lifecycle(initial = Available)]
#[serde(rename_all = "kebab-case")]
pub enum BicycleStatus {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
}
