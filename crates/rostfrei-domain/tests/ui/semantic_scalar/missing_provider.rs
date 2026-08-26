use domain::{DomainIdentity, ValueObject};

struct Custom;

#[derive(ValueObject)]
#[domain(id = "missing-field", label = "Missing field", owner = Owner)]
struct MissingField(#[domain(scalar)] Custom);

#[derive(DomainIdentity)]
#[domain(owner = Entity, scalar)]
struct MissingIdentity(Custom);

fn main() {}
