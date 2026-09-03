#[derive(BoundedContext)]
#[domain(id = "bike-rental", label = "Bike rental")]
pub struct BikeRental;

pub const STREAM_PREFIX: &str = "rental";
