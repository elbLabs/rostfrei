use rostfrei_domain::DomainError;

#[derive(DomainError)]
#[domain(id = "choice", label = "Choice", owner = Owner, code = "CHOICE", message = "Choice.")]
enum Choice {
    First,
}

#[derive(DomainError)]
#[domain(id = "storage", label = "Storage", owner = Owner, code = "STORAGE", message = "Storage.")]
union Storage {
    integer: u64,
    decimal: f64,
}

#[derive(DomainError)]
#[domain(id = "generic", label = "Generic", owner = Owner, code = "GENERIC", message = "Generic.")]
struct Generic<T>(T);

fn main() {}
