use domain::domain_decisions;

#[domain_decisions(entity)]
trait Decisions {
    const ENABLED: bool;
}

fn main() {}
