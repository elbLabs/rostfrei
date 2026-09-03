#[derive(AggregateEvents)]
pub enum RentalFleetEvents {
    BicycleRented(BicycleRented),
}
