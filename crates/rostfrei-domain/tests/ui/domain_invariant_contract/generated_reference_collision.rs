use domain::{InvariantReference, domain_invariants};

#[domain_invariants]
trait Rules {
    const __DOMAIN_INVARIANT_REFERENCE_VALID: InvariantReference =
        InvariantReference::__from_local("other");

    #[invariant(id = "valid", label = "Valid")]
    fn valid();
}

fn main() {}
