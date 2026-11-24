mod finance;
mod model;
mod rental;
#[cfg(test)]
mod tests;
mod workshop;

pub use finance::{Finance, rental_payment};
pub use model::domain_model;
pub use rental::{Rental, bicycle_transfer, rental_fleet, rental_tariff};
pub use workshop::{Workshop, repair_job};
