use rostfrei::DomainIdentity;
use serde::{Deserialize, Serialize};

#[derive(
    DomainIdentity, Clone, Debug, Deserialize, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize,
)]
#[serde(try_from = "String")]
pub struct BicycleId(String);

impl BicycleId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.trim().is_empty() && value.trim() == value).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BicycleId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("bicycle ID must be non-empty and trimmed")
    }
}
