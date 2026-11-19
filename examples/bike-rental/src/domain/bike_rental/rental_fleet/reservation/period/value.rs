use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::codec::PeriodInput;

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-period", label = "Rental period")]
#[serde(try_from = "PeriodInput")]
pub struct RentalPeriod {
    #[serde(with = "time::serde::rfc3339")]
    starts_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    ends_at: OffsetDateTime,
}

impl RentalPeriod {
    pub fn new(starts_at: &str, ends_at: &str) -> Result<Self, &'static str> {
        let starts_at = OffsetDateTime::parse(starts_at, &Rfc3339)
            .map_err(|_| "rental start must be an RFC 3339 timestamp")?;
        let ends_at = OffsetDateTime::parse(ends_at, &Rfc3339)
            .map_err(|_| "rental end must be an RFC 3339 timestamp")?;
        if starts_at >= ends_at {
            return Err("rental end must be after its start");
        }
        Ok(Self { starts_at, ends_at })
    }

    pub const fn starts_at(&self) -> OffsetDateTime {
        self.starts_at
    }
    pub const fn ends_at(&self) -> OffsetDateTime {
        self.ends_at
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.starts_at < other.ends_at && other.starts_at < self.ends_at
    }
}
