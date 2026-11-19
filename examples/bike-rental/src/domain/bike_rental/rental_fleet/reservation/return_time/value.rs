use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "return-time", label = "Return time")]
#[serde(transparent)]
pub struct ReturnTime(#[serde(with = "time::serde::rfc3339")] OffsetDateTime);

impl ReturnTime {
    pub fn new(value: &str) -> Result<Self, time::error::Parse> {
        OffsetDateTime::parse(value, &Rfc3339).map(Self)
    }
    pub const fn instant(&self) -> OffsetDateTime {
        self.0
    }
}
