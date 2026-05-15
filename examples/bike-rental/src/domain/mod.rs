mod bike_rental;
mod model;
#[cfg(test)]
mod tests;

pub use bike_rental::{BikeRental, bicycle_transfer, rental_fleet};
pub use model::domain_model;
