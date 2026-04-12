use rostfrei_domain::domain_decisions;

#[domain_decisions(entity)]
trait Decisions
where
    Self: Send,
{
}

fn main() {}
