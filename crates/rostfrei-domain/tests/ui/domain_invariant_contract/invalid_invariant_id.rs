use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "Invalid", label = "Valid")]
    fn valid();
}

fn main() {}
