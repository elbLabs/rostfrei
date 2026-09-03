#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
pub trait RentBicycleContract {
    fn rent_bicycle(&mut self);
}

pub trait InfrastructureConfiguration {
    fn stream_name(&self) -> &str;
}
