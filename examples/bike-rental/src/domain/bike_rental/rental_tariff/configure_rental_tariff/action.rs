use rostfrei::domain_action;

use super::InvalidRentalTariff;
use crate::domain::rental_tariff::RentalTariffTerms;

#[domain_action(id = "configure-rental-tariff", label = "Configure rental tariff")]
pub trait ConfigureRentalTariffAction {
    fn configure_rental_tariff(
        &mut self,
        terms: RentalTariffTerms,
    ) -> Result<(), InvalidRentalTariff>;
}
