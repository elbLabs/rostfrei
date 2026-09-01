use domain::domain_invariants;

#[domain_invariants]
trait Rules {
    #[invariant(id = "valid", label = "Valid")]
    fn first();
    #[invariant(id = "valid", label = "Also valid")]
    fn second();
}

fn main() {}
