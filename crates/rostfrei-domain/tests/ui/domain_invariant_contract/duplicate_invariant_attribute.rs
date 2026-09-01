use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid", label = "Valid")]
    #[invariant(id = "other", label = "Other")]
    fn valid();
}

fn main() {}
