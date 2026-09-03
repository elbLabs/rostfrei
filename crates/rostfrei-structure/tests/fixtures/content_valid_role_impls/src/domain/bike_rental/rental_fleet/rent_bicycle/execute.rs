impl RentBicycleContract for AggregateInstance<RentalFleetAggregate> {
    fn rent_bicycle(&mut self, bicycle_id: &str) {
        let _ = normalize_bicycle_id(bicycle_id);
    }
}

fn normalize_bicycle_id(input: &str) -> &str {
    input.trim()
}
