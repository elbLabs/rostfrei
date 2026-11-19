use crate::domain::rental_fleet::{RentalPeriod, ReturnTime};
use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-return", label = "Rental return")]
pub struct RentalReturn {
    pub returned_at: Option<ReturnTime>,
    pub damaged: bool,
}

impl RentalReturn {
    pub fn permits_full_refund(&self, period: &RentalPeriod) -> bool {
        !self.damaged
            && self.returned_at.as_ref().is_some_and(|time| {
                time.instant() >= period.starts_at() && time.instant() <= period.ends_at()
            })
    }
}
