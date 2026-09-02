use domain::domain_decision;

#[domain_decision(id = "missing-label")]
trait MissingLabel {
    fn decide();
}

fn main() {}
