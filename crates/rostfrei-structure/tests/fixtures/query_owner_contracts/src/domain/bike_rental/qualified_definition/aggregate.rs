#[derive(Aggregate)]
pub struct QualifiedAggregate;

impl AggregateDefinition for QualifiedAggregate {
    type Context = BikeRental;
    type Root = domain::RentalFleet;
    type Event = QualifiedEvents;
}
