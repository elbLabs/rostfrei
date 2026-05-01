#[derive(Aggregate)]
pub struct RentalFleetAggregate;

impl AggregateDefinition for RentalFleetAggregate {
    type Context = BikeRental;
    type Root = RentalFleet;
    type Event = RentalFleetEvents;
}
