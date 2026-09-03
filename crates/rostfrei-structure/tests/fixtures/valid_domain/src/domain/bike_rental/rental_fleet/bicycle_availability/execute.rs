impl BicycleAvailabilityQuery for RentalFleet {
    fn bicycle_availability(&self, _input: BicycleId) -> BicycleAvailability {
        BicycleAvailability::Available
    }
}
