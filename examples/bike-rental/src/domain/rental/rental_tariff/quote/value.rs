use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-quote", label = "Rental quote")]
pub struct RentalQuote {
    pub currency: String,
    pub rental_charge_minor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_deposit_minor: Option<u64>,
}

impl RentalQuote {
    pub fn total_due_minor(&self) -> Option<u64> {
        self.rental_charge_minor
            .checked_add(self.security_deposit_minor?)
    }
}
