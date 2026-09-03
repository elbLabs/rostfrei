#[derive(DomainEvent)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented;

pub static EVENT_SUBJECT: &str = "bicycle-rented";
