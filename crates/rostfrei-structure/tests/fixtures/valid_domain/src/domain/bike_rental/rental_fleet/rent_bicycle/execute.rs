impl RentBicycleContract for AggregateInstance<RentalFleetAggregate> {
    fn rent_bicycle(&mut self, bicycle_id: BicycleId) -> Result<(), BicycleUnavailable> {
        self.raise(BicycleRented { bicycle_id });
        Ok(())
    }
}
