use rostfrei::{Aggregate, AggregateDefinition};

use super::{RentalFleet, RentalFleetEvent};
use crate::domain::Rental;

#[derive(Aggregate)]
#[domain(id = "rental-fleet", label = "Rental fleet")]
pub struct RentalFleetAggregate;

impl AggregateDefinition for RentalFleetAggregate {
    const ANCHOR: bool = true;

    type Context = Rental;
    type Root = RentalFleet;
    type Event = RentalFleetEvent;
}
