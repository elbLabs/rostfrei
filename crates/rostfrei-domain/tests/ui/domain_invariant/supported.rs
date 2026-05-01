use domain::{InvariantDescriptor, InvariantId, InvariantViolation, domain_invariant};

#[domain_invariant(id = "valid", label = "Valid")]
trait Valid {
    fn validate(&self) -> Option<InvariantViolation>;
}

struct Candidate;

impl Valid for Candidate {
    fn validate(&self) -> Option<InvariantViolation> {
        None
    }
}

fn main() {
    let descriptor: InvariantDescriptor = <Candidate as Valid>::DESCRIPTOR;
    assert_eq!(descriptor.id, InvariantId("valid"));
    assert!(Candidate.validate().is_none());
}
rostfrei_domain_macros::__install_test_macro_support!();
