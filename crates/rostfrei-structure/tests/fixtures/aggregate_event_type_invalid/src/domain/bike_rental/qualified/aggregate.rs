#[derive(Aggregate)]
pub struct QualifiedAggregate;

impl AggregateDefinition for QualifiedAggregate {
    type Context = BikeRental;
    type Root = Qualified;
    type Event = event_set::QualifiedEvents;
}
