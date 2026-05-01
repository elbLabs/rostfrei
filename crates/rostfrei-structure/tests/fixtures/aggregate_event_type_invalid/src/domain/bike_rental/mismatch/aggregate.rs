#[derive(Aggregate)]
pub struct MismatchAggregate;

impl AggregateDefinition for MismatchAggregate {
    type Context = BikeRental;
    type Root = Mismatch;
    type Event = OtherEvents;
}
