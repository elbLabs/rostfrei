use domain::DomainError;

#[derive(DomainError)]
#[domain(id = "denied", label = "Denied", owner = Owner, code = "DENIED", message = "Denied.")]
struct Denied {
    #[domain(field)]
    value: String,
}

fn main() {}
