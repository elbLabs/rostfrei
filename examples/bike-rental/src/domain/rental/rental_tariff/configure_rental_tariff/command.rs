use rostfrei::Command;

use crate::domain::{Rental, rental_fleet::BicycleCategory, rental_tariff::RentalTariffTerms};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "configure-rental-tariff", label = "Configure rental tariff", context = Rental)]
pub struct ConfigureRentalTariff {
    pub category: BicycleCategory,
    pub terms: RentalTariffTerms,
}
