use domain::domain_invariant;

#[domain_invariant(label = "Missing ID")]
trait MissingId {
    fn validate();
}

fn main() {}
