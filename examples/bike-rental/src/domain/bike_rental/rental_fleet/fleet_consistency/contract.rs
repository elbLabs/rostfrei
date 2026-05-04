use rostfrei::{InvariantViolation, domain_invariant};

use crate::domain::rental_fleet::RentalFleet;

#[domain_invariant(
    id = "unique-bicycle-identities",
    label = "Bicycle identities are unique"
)]
pub(in crate::domain) trait FleetConsistency {
    fn unique_bicycle_identities(candidate: &RentalFleet) -> Option<InvariantViolation>;
}
