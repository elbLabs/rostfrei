use rostfrei::{Aggregate, AggregateDefinition};

use super::{RentalTariff, RentalTariffEvent};
use crate::domain::BikeRental;

#[derive(Aggregate)]
#[domain(id = "rental-tariff", label = "Rental tariff")]
pub struct RentalTariffAggregate;

impl AggregateDefinition for RentalTariffAggregate {
    type Context = BikeRental;
    type Root = RentalTariff;
    type Event = RentalTariffEvent;
}
