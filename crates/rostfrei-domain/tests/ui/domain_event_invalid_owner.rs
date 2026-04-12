use domain::DomainEvent;

struct Owner;

#[derive(DomainEvent)]
#[domain(id = "created", label = "Created", owner = Owner)]
struct Created;

fn main() {}
