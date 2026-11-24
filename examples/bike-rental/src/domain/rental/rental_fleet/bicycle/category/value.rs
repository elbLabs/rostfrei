use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-category", label = "Bicycle category")]
#[serde(try_from = "String")]
pub struct BicycleCategory(String);

impl BicycleCategory {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty() && value.trim() == value).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
