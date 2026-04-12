use domain::DomainEvent;

#[derive(DomainEvent)]
#[domain(id = "choice", label = "Choice", owner = Owner)]
enum Choice {
    First,
}

#[derive(DomainEvent)]
#[domain(id = "storage", label = "Storage", owner = Owner)]
union Storage {
    integer: u64,
    decimal: f64,
}

#[derive(DomainEvent)]
#[domain(id = "generic", label = "Generic", owner = Owner)]
struct Generic<T>(T);

fn main() {}
