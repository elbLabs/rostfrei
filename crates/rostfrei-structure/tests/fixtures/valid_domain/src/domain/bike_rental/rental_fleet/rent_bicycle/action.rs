#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
pub trait RentBicycleContract {
    fn rent_bicycle(&mut self, bicycle_id: BicycleId) -> Result<(), BicycleUnavailable>;
}
