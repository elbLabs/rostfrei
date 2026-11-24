use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::domain::rental_tariff::{RentalTariffId, RentalTariffTerms};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-tariff-configured", label = "Rental tariff configured")]
pub struct RentalTariffConfigured {
    pub tariff_id: RentalTariffId,
    pub terms: RentalTariffTerms,
}
