use rostfrei::{InvariantOwnerType, InvariantViolation, domain_invariants};

#[allow(
    non_snake_case,
    reason = "domain_invariants currently generates a doc-hidden uppercase method"
)]
#[domain_invariants(aggregate)]
pub(in crate::domain::bike_rental::rental_fleet) trait FleetConsistency {
    #[invariant(
        id = "unique-bicycle-identities",
        label = "Bicycle identities are unique"
    )]
    fn unique_bicycle_identities(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}
