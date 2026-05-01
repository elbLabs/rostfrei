#[domain_invariant(id = "unique-bicycle-identities", label = "Unique bicycle identities")]
pub trait FleetConsistency {
    fn unique_bicycle_identities(candidate: &RentalFleet) -> Option<InvariantViolation>;
}
