impl RentBicycleContract for AggregateInstance<RentalFleetAggregate> {
    fn rent_bicycle(&mut self) {}
}

pub fn normalize_bicycle_id(input: &str) -> &str {
    input.trim()
}
