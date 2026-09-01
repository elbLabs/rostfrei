use rostfrei::{InvariantOwnerType, InvariantViolation, domain_invariants};

#[allow(
    dead_code,
    non_snake_case,
    reason = "the invariant remains an explicit contract while aggregate fanout is deferred; domain_invariants also generates a doc-hidden uppercase method"
)]
#[domain_invariants(aggregate)]
pub(in crate::domain) trait FleetConsistency {
    #[invariant(
        id = "unique-bicycle-identities",
        label = "Bicycle identities are unique"
    )]
    fn unique_bicycle_identities(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}
