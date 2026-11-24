use rostfrei::{Aggregate, AggregateDefinition};

use super::{RepairJob, RepairJobEvent};
use crate::domain::Workshop;

#[derive(Aggregate)]
#[domain(id = "repair-job", label = "Repair job")]
pub struct RepairJobAggregate;

impl AggregateDefinition for RepairJobAggregate {
    const ANCHOR: bool = true;

    type Context = Workshop;
    type Root = RepairJob;
    type Event = RepairJobEvent;
}
