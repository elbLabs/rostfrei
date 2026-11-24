use rostfrei::domain_query;

use super::BicycleAvailability;
use crate::domain::rental::rental_fleet::BicycleId;

#[domain_query(id = "bicycle-availability", label = "Bicycle availability")]
pub trait BicycleAvailabilityQuery {
    fn bicycle_availability(&self, input: &BicycleId) -> Option<BicycleAvailability>;
}
