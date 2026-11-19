use rostfrei::AggregateInstance;

use super::{ConfigureRentalTariffAction, InvalidRentalTariff, RentalTariffConfigured};
use crate::domain::rental_tariff::{RentalTariffAggregate, RentalTariffTerms};

impl ConfigureRentalTariffAction for AggregateInstance<RentalTariffAggregate> {
    fn configure_rental_tariff(
        &mut self,
        terms: RentalTariffTerms,
    ) -> Result<(), InvalidRentalTariff> {
        if !terms.is_valid() {
            return Err(InvalidRentalTariff {
                tariff_id: self.state().tariff_id().clone(),
            });
        }
        if self.state().terms() == Some(&terms) {
            return Ok(());
        }
        self.raise(RentalTariffConfigured {
            tariff_id: self.state().tariff_id().clone(),
            terms,
        });
        Ok(())
    }
}
