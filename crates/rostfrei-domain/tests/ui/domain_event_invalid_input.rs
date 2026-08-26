use domain::DomainEvent;

#[derive(DomainEvent)]
#[domain(id = "choice", label = "Choice")]
enum Choice {
    First,
}

#[derive(DomainEvent)]
#[domain(id = "storage", label = "Storage")]
union Storage {
    integer: u64,
    decimal: f64,
}

#[derive(DomainEvent)]
#[domain(id = "generic", label = "Generic")]
struct Generic<T>(T);

fn main() {}
