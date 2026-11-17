use rostfrei::Command;

use crate::domain::{BikeRental, rental_fleet::BicycleCategory, rental_tariff::RentalTariffTerms};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "configure-rental-tariff", label = "Configure rental tariff", context = BikeRental)]
pub struct ConfigureRentalTariff {
    pub category: BicycleCategory,
    pub terms: RentalTariffTerms,
}
