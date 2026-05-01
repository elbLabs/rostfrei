use rostfrei::BoundedContext;

#[derive(BoundedContext)]
#[domain(id = "bike-rental", label = "Bike Rental")]
pub struct BikeRental;
