use domain::{InvariantDescriptor, domain_invariants};

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(candidate: &u8) -> bool;
}

struct Policy;

impl Rules for Policy {
    fn valid(candidate: &u8) -> bool {
        *candidate > 0
    }
}

const _: &'static [InvariantDescriptor] = <Policy as Rules>::__DOMAIN_INVARIANTS;

fn main() {}
