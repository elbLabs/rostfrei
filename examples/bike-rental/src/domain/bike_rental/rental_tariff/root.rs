use rostfrei::{Entity, EntityDefinition};

use super::{RentalTariffAggregate, RentalTariffId, RentalTariffTerms};

#[derive(Entity, Debug)]
#[domain(id = "rental-tariff-root", label = "Rental tariff")]
pub struct RentalTariff {
    pub(super) tariff_id: RentalTariffId,
    pub(super) terms: Option<RentalTariffTerms>,
}

impl EntityDefinition for RentalTariff {
    type Owner = RentalTariffAggregate;
    type Identity = RentalTariffId;

    fn identity(&self) -> &Self::Identity {
        &self.tariff_id
    }
}

impl RentalTariff {
    pub const fn tariff_id(&self) -> &RentalTariffId {
        &self.tariff_id
    }
    pub const fn terms(&self) -> Option<&RentalTariffTerms> {
        self.terms.as_ref()
    }
}
