use rostfrei_domain::DomainEvent;

#[derive(DomainEvent)]
#[domain(id = "created", label = "Created", owner = Owner)]
struct Created {
    #[domain(field)]
    value: String,
}

fn main() {}
