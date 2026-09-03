#[domain_query(id = "bicycle-availability", label = "Bicycle availability")]
pub trait BicycleAvailabilityQuery {
    fn bicycle_availability(&self, input: BicycleId) -> BicycleAvailability;
}
