use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

use crate::domain::{rental_fleet::RentalPeriod, rental_tariff::RentalQuote};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-tariff-terms", label = "Rental tariff terms")]
pub struct RentalTariffTerms {
    pub currency: String,
    pub hourly_rate_minor: u64,
    pub daily_cap_minor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_deposit_minor: Option<u64>,
}

impl RentalTariffTerms {
    pub fn is_valid(&self) -> bool {
        self.currency.len() == 3
            && self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            && self.hourly_rate_minor > 0
            && self.daily_cap_minor > 0
            && self.security_deposit_minor.is_some()
    }

    pub fn quote(&self, period: &RentalPeriod) -> Option<RentalQuote> {
        if !self.is_valid() {
            return None;
        }
        let nanos = period
            .ends_at()
            .unix_timestamp_nanos()
            .checked_sub(period.starts_at().unix_timestamp_nanos())?;
        let hours = u128::try_from(nanos).ok()?.div_ceil(3_600_000_000_000);
        let hourly = u128::from(self.hourly_rate_minor);
        let cap = u128::from(self.daily_cap_minor);
        let full_day = hourly.checked_mul(24)?.min(cap);
        let complete_days = (hours / 24).checked_mul(full_day)?;
        let remainder = (hours % 24).checked_mul(hourly)?.min(cap);
        let total = complete_days.checked_add(remainder)?;
        Some(RentalQuote {
            currency: self.currency.clone(),
            rental_charge_minor: u64::try_from(total).ok()?,
            security_deposit_minor: self.security_deposit_minor,
        })
    }
}
