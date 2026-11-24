use rostfrei::{Initialize, StreamId};

use super::{RentalTariff, RentalTariffAggregate, RentalTariffId};

impl Initialize<RentalTariffAggregate> for RentalTariff {
    fn initialize(stream: &StreamId) -> Self {
        Self {
            tariff_id: RentalTariffId::from(stream.aggregate_id()),
            terms: None,
        }
    }
}
