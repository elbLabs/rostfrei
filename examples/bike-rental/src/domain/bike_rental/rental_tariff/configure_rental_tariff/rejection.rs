use rostfrei::DomainError;

use crate::domain::rental_tariff::RentalTariffId;

#[derive(DomainError, Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[domain(
    id = "invalid-rental-tariff",
    label = "Invalid rental tariff",
    code = "INVALID_RENTAL_TARIFF",
    message = "A tariff needs a three-letter uppercase currency, positive hourly rate and daily cap, and a security deposit amount."
)]
pub struct InvalidRentalTariff {
    pub tariff_id: RentalTariffId,
}
