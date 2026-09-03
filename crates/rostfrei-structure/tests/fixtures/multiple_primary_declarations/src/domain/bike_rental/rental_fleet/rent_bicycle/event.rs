#[derive(DomainEvent)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented;

#[derive(DomainEvent)]
#[domain(id = "rental-recorded", label = "Rental recorded")]
pub struct RentalRecorded;
