use domain::{DomainIdentity, ValueObject};

struct Custom;

#[derive(ValueObject)]
#[domain(id = "missing-field", label = "Missing field", owner = Owner)]
struct MissingField(#[domain(scalar)] Custom);

#[derive(DomainIdentity)]
struct MissingIdentity(Custom);

fn main() {}
