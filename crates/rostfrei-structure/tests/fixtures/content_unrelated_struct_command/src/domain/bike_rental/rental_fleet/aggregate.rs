#[derive(Aggregate)]
#[domain(id = "rental-fleet", label = "Rental fleet")]
pub struct RentalFleetAggregate;

impl AggregateDefinition for RentalFleetAggregate {
    type Context = BikeRental;
    type Root = RentalFleet;
    type Event = RentalFleetEvents;
}
