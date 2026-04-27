use domain::domain_invariants;

#[domain_invariants(entity)]
trait Rules {
    #[invariant(id = "valid", label = "Valid")]
    fn valid();
}

fn main() {}
