use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid", id = "other", label = "Valid")]
    fn valid();
}

fn main() {}
