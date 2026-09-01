use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid", label = " ")]
    fn valid();
}

fn main() {}
