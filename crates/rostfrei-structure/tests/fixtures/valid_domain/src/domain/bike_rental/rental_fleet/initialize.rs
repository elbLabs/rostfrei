impl Initialize<RentalFleetAggregate> for RentalFleet {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            fleet_id: FleetId(stream_id.to_string()),
            bicycles: Vec::new(),
        }
    }
}
