use super::{InvariantOwnerId, InvariantViolation};

pub trait InvariantOwnerType: 'static {
    type Candidate: 'static;

    const INVARIANT_OWNER_ID: InvariantOwnerId;

    fn validate_invariants(candidate: &Self::Candidate) -> Result<(), Vec<InvariantViolation>> {
        let _ = candidate;
        Ok(())
    }
}
