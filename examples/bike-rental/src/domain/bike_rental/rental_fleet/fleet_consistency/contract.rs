use rostfrei::{InvariantViolation, domain_invariant};

use crate::domain::rental_fleet::RentalFleet;

#[allow(
    dead_code,
    reason = "invariant fanout is intentionally absent; tests exercise the authored contract directly"
)]
#[domain_invariant(
    id = "unique-bicycle-identities",
    label = "Bicycle identities are unique"
)]
pub(in crate::domain) trait FleetConsistency {
    fn unique_bicycle_identities(candidate: &RentalFleet) -> Option<InvariantViolation>;
}
