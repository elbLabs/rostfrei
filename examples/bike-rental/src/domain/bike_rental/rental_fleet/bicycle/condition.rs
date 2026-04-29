use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-condition", label = "Bicycle condition")]
#[serde(rename_all = "kebab-case")]
pub enum BicycleCondition {
    Serviceable,
    MaintenanceRequired,
}
