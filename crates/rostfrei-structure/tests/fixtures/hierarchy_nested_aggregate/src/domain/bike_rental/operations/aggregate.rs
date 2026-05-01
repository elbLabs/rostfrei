#[derive(Aggregate)]
#[domain(id = "operations", label = "Operations")]
pub struct OperationsAggregate;

impl AggregateDefinition for OperationsAggregate {
    type Context = BikeRental;
    type Root = Operations;
    type Event = RentalFleetEvents;
}
