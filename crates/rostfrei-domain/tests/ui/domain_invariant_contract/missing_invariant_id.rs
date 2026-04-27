use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(label = "Valid")]
    fn valid();
}

fn main() {}
