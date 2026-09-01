use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid", label = "Valid")]
    const VALID: bool;
}

fn main() {}
