use rostfrei::Apply;

use super::RentalTariffConfigured;
use crate::domain::rental_tariff::RentalTariff;

impl Apply<RentalTariffConfigured> for RentalTariff {
    fn apply(&mut self, event: &RentalTariffConfigured) {
        self.terms = Some(event.terms.clone());
    }
}
