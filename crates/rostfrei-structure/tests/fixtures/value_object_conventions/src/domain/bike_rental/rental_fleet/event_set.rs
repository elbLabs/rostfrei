#[derive(AggregateEvents)]
pub enum RentalFleetEvents {
    Changed(RentalFleetChanged),
}
