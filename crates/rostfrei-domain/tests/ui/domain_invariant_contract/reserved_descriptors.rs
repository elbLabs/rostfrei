use domain::{InvariantDescriptor, domain_invariants};

#[domain_invariants]
trait Rules {
    const __DOMAIN_INVARIANTS: &'static [InvariantDescriptor] = &[];

    #[invariant(id = "valid", label = "Valid")]
    fn valid();
}

fn main() {}
