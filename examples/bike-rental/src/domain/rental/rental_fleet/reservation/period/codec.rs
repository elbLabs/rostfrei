use serde::Deserialize;

use super::RentalPeriod;

#[derive(Deserialize)]
pub(super) struct PeriodInput {
    starts_at: String,
    ends_at: String,
}

impl TryFrom<PeriodInput> for RentalPeriod {
    type Error = &'static str;

    fn try_from(value: PeriodInput) -> Result<Self, Self::Error> {
        Self::new(&value.starts_at, &value.ends_at)
    }
}
