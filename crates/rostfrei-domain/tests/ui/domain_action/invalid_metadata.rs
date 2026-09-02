use domain::domain_action;

#[domain_action(id = "missing-label")]
trait MissingLabel {
    fn execute();
}

fn main() {}
