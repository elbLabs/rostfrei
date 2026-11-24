use rostfrei::{Aggregate, AggregateDefinition};

use super::{RepairJob, RepairJobEvent};
use crate::domain::BikeRental;

#[derive(Aggregate)]
#[domain(id = "repair-job", label = "Repair job")]
pub struct RepairJobAggregate;

impl AggregateDefinition for RepairJobAggregate {
    const ANCHOR: bool = true;

    type Context = BikeRental;
    type Root = RepairJob;
    type Event = RepairJobEvent;
}
