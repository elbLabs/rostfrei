use rostfrei::BoundedContext;

pub mod rental_fleet;

#[derive(BoundedContext)]
#[domain(id = "bike-rental", label = "Bike Rental")]
pub struct BikeRental;
