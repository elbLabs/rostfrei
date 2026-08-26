use domain::DomainEvent;

#[derive(DomainEvent)]
#[domain(id = "created", label = "Created")]
struct Created {
    #[domain(field)]
    value: String,
}

fn main() {}
