use domain::DomainError;

#[derive(DomainError)]
#[domain(id = "denied", label = "Denied", code = "DENIED", message = "Denied.")]
struct Denied {
    #[domain(field)]
    value: String,
}

fn main() {}
