use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid", label = "Valid", owner = Rules)]
    fn valid();
}

fn main() {}
