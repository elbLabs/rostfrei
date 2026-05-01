#[derive(Aggregate)]
pub struct RentalFleetAggregate;

impl AggregateDefinition for RentalFleetAggregate {
    type Event = RentalFleetEvents;
}
