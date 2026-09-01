use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid")]
    fn valid();
}

fn main() {}
